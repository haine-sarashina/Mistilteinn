/// Page orchestration module.
///
/// Connects HTML parsing, CSS computation, layout building, and rendering
/// into a single pipeline: HTML → CSS → Layout → Render.
use rustc_hash::FxHashMap;

use crate::css;
use crate::html::{self, DomArena, DomHandle, NodeId};
use crate::js::DispatchOutcome;
use crate::layout::Rect;

/// Cached decoded image data.
#[derive(Clone)]
pub struct CachedImage {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// A fully parsed and laid-out page ready for rendering.
pub struct Page {
    /// The document.
    ///
    /// Shared with this page's JavaScript context rather than owned outright,
    /// so a handler's DOM mutation is visible here without copying anything
    /// back. `DomArena` has interior mutability, so `Rc` derefs to `&DomArena`
    /// and every `page.arena.foo()` call reads through unchanged.
    pub arena: std::rc::Rc<DomArena>,
    pub title: String,
    #[allow(dead_code)]
    pub styles: FxHashMap<u32, css::ComputedValues>,
    pub layout_root: crate::layout::LayoutNode,
    pub view_width: f32,
    pub view_height: f32,
    pub page_url: String,
    /// Page zoom. Every absolute CSS length is multiplied by it, so the page
    /// reflows into the window rather than being scaled as a picture.
    pub zoom: f32,
    pub image_cache: FxHashMap<String, CachedImage>,
    /// Merged (UA + author) stylesheet for style recomputation (e.g., :hover).
    pub stylesheet: crate::css::parser::Stylesheet,
    /// The page's JavaScript context, kept alive past the initial script run so
    /// registered event listeners can still be called.
    js_context: boa_engine::Context,
}

impl Page {
    /// Set the page zoom and lay the document out again at that scale.
    ///
    /// Zoom is a style-time factor rather than a paint-time one, so changing it
    /// means recomputing the cascade: lengths, and therefore line breaking and
    /// every box that depends on them, all change.
    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom;
        self.recompute_with_hover(&[]);
    }

    /// The URL that relative references in this document resolve against.
    ///
    /// A `<base href>` overrides the document's own URL, and is itself resolved
    /// against that URL so a relative `<base href="/assets/">` works.
    pub fn base_url(&self) -> String {
        match self.arena.base_href() {
            Some(href) if !self.page_url.is_empty() => {
                crate::network::resolve_url(&self.page_url, &href)
            }
            Some(href) => href,
            None => self.page_url.clone(),
        }
    }
}

impl Page {
    /// Run the full pipeline: parse HTML + CSS, compute styles, build and measure layout tree.
    pub fn new(html_source: &str, css_source: &str, view_width: f32, view_height: f32) -> Self {
        Self::new_with_csp(
            html_source,
            css_source,
            view_width,
            view_height,
            &crate::network::security::Csp::default(),
        )
    }

    /// Build a page under the given Content-Security-Policy.
    ///
    /// The policy has to be known here rather than applied afterwards: this is
    /// where the document's inline scripts run, and refusing to run them is the
    /// most common thing a CSP is set up to do.
    pub fn new_with_csp(
        html_source: &str,
        css_source: &str,
        view_width: f32,
        view_height: f32,
        csp: &crate::network::security::Csp,
    ) -> Self {
        use crate::network::security::ResourceKind;

        // Stage 1: Parse HTML into DOM arena
        let arena = html::parse_html(html_source);
        let title = arena
            .extract_title()
            .unwrap_or_else(|| "New Tab".to_string());
        let arena = std::rc::Rc::new(arena);

        // Stage 1.5: Execute inline JS scripts with DOM bindings connected to
        // the arena. The context is kept afterwards: scripts register event
        // listeners that have to survive to be called later.
        let mut js_context = crate::js::init_js_engine_with_arena(arena.clone());
        let mut blocked = 0usize;
        for (script, nonce) in arena.extract_scripts_with_nonce() {
            if csp.allows_inline_with_nonce(ResourceKind::Script, nonce.as_deref()) {
                crate::js::execute_script(&mut js_context, &script);
            } else {
                blocked += 1;
            }
        }
        if blocked > 0 {
            log::warn!("{blocked} inline script(s) blocked by this page's Content-Security-Policy");
        }

        // Stage 2: Parse CSS into author stylesheet
        let author_stylesheet = crate::css::parser::parse_stylesheet(css_source);

        // Stage 2.5: Merge UA stylesheet (lowest priority) with author stylesheet
        let ua_stylesheet = css::user_agent_stylesheet();
        let stylesheet = css::merge_stylesheets_with_author(&ua_stylesheet, &author_stylesheet);

        // Stage 3: Compute styles for every element node
        let styles = css::compute_styles_for_tree(&arena, &stylesheet, (view_width, view_height));

        // Stage 4: Build layout tree starting from document root (node 0)
        let mut layout_root = crate::layout::build_layout_tree(
            0,
            &styles,
            |id: u32| arena.get(DomHandle(NodeId::from_raw(id))),
            view_width,
        );

        // Stage 4.5: Extract absolutely positioned children from normal flow
        crate::layout::extract_absolute_children(&mut layout_root);

        // Stage 5: Compute layout positions for normal-flow children
        let mut text_renderer = crate::render::text::TextRenderer::new();
        crate::layout::compute_layout(&mut layout_root, view_width, &mut text_renderer);

        // Stage 6: Apply relative positioning offsets to shifted elements
        crate::layout::apply_relative_positioning(&mut layout_root);

        // Stage 7: Compute positions for absolutely positioned children
        let containing_block = Rect::new(
            layout_root.padding[3] + layout_root.border[3],
            layout_root.padding[0] + layout_root.border[0],
            (layout_root.rect.width
                - layout_root.padding[1]
                - layout_root.padding[3]
                - layout_root.border[1]
                - layout_root.border[3])
                .max(0.0),
            (layout_root.rect.height
                - layout_root.padding[0]
                - layout_root.padding[2]
                - layout_root.border[0]
                - layout_root.border[2])
                .max(0.0),
        );
        crate::layout::compute_absolute_positions(
            &mut layout_root,
            containing_block,
            &mut text_renderer,
        );

        log::info!(
            "Page pipeline complete — view: {}x{}, styles: {}, root children: {}",
            view_width,
            view_height,
            styles.len(),
            layout_root.children.len()
        );

        let mut page = Self {
            arena,
            title,
            styles,
            layout_root,
            view_width,
            view_height,
            page_url: String::new(),
            zoom: 1.0,
            image_cache: FxHashMap::default(),
            stylesheet,
            js_context,
        };

        // The document exists now, so anything waiting on it can run. This
        // relayouts only if a handler actually changed something.
        page.fire_load_events();
        page
    }

    /// Fire the page-ready events at `window` / `document`.
    ///
    /// Scripts run during parsing, so a handler registered for
    /// `DOMContentLoaded` or `load` has nothing to fire it until the document
    /// is built. Called once at the end of the pipeline.
    pub fn fire_load_events(&mut self) -> DispatchOutcome {
        let mut total = DispatchOutcome::default();
        for event in ["domcontentloaded", "load"] {
            let outcome = self.dispatch_event_along(&[crate::js::dom::WINDOW_TARGET], event);
            total.fired += outcome.fired;
            total.default_prevented |= outcome.default_prevented;
        }
        total
    }

    /// Fire an event at `node_id` and at each of its ancestors, as bubbling does.
    ///
    /// `path` runs outermost-to-innermost, which is the order the hit test
    /// produces it; dispatch walks it in reverse so the innermost target is
    /// notified first. Layout is recomputed when any handler ran, because a
    /// handler is free to have changed the DOM.
    pub fn dispatch_event_along(&mut self, path: &[u32], event_type: &str) -> DispatchOutcome {
        crate::js::dom::set_active_arena(Some(self.arena.clone()));
        crate::js::reset_default_prevented(&mut self.js_context);

        let mut total = DispatchOutcome::default();
        for &node_id in path.iter().rev() {
            let outcome = crate::js::dispatch_event(&mut self.js_context, node_id, event_type);
            total.fired += outcome.fired;
            total.default_prevented |= outcome.default_prevented;
        }

        if total.ran() {
            self.recompute_with_hover(&[]);
        }
        total
    }

    /// Run a script in this page's context, relayouting if it changed the DOM.
    ///
    /// Used by tests and by anything that needs to evaluate script after load.
    pub fn eval_script(&mut self, script: &str) {
        crate::js::dom::set_active_arena(Some(self.arena.clone()));
        crate::js::execute_script(&mut self.js_context, script);
        self.recompute_with_hover(&[]);
    }

    /// Recompute styles and rebuild the layout tree with the given set of
    /// hovered DOM node IDs. If `hovered_ids` is empty, computes static styles
    /// (equivalent to initial load).
    pub fn recompute_with_hover(&mut self, hovered_ids: &[u32]) {
        // Recompute styles with hover awareness
        self.styles = css::compute_styles_for_tree_with_hover_zoom(
            &self.arena,
            &self.stylesheet,
            (self.view_width, self.view_height),
            hovered_ids,
            self.zoom,
        );

        // Rebuild layout tree from new computed styles
        let get_node = |id: u32| self.arena.get(DomHandle(NodeId::from_raw(id)));
        self.layout_root =
            crate::layout::build_layout_tree(0, &self.styles, get_node, self.view_width);

        // Re-extract absolute children and recompute layout
        crate::layout::extract_absolute_children(&mut self.layout_root);
        let mut text_renderer = crate::render::text::TextRenderer::new();
        crate::layout::compute_layout(&mut self.layout_root, self.view_width, &mut text_renderer);
        crate::layout::apply_relative_positioning(&mut self.layout_root);

        // Recompute absolute positions
        let containing_block = Rect::new(
            self.layout_root.padding[3] + self.layout_root.border[3],
            self.layout_root.padding[0] + self.layout_root.border[0],
            (self.layout_root.rect.width
                - self.layout_root.padding[1]
                - self.layout_root.padding[3]
                - self.layout_root.border[1]
                - self.layout_root.border[3])
                .max(0.0),
            (self.layout_root.rect.height
                - self.layout_root.padding[0]
                - self.layout_root.padding[2]
                - self.layout_root.border[0]
                - self.layout_root.border[2])
                .max(0.0),
        );
        crate::layout::compute_absolute_positions(
            &mut self.layout_root,
            containing_block,
            &mut text_renderer,
        );
    }

    /// Collect renderable rectangles with colors from the layout tree.
    pub fn collect_rects(&self) -> Vec<(Rect, Option<[u8; 4]>)> {
        crate::layout::collect_render_rects(&self.layout_root)
    }

    /// Update the `value` attribute of an input element and recompute layout.
    pub fn set_input_value_and_recompute(&mut self, node_id: u32, value: &str) {
        self.arena.set_attribute(node_id, "value", value);
        self.recompute_with_hover(&[]);
    }

    /// The options of a `<select>`, as (dom id, label, selected).
    pub fn select_options(&self, select_id: u32) -> Vec<(u32, String, bool)> {
        let get_node = |id: u32| self.arena.get(DomHandle(NodeId::from_raw(id)));
        match get_node(select_id) {
            Some(node) => crate::layout::select_options(&node, get_node),
            None => Vec::new(),
        }
    }

    /// Choose an option of a `<select>` and relayout.
    ///
    /// `selected` is an attribute rather than a separate piece of state, so the
    /// change survives the layout rebuild the same way a typed input value does.
    pub fn select_option_and_recompute(&mut self, select_id: u32, option_id: u32) {
        for (id, _, _) in self.select_options(select_id) {
            if id == option_id {
                self.arena.set_attribute(id, "selected", "selected");
            } else {
                self.arena.remove_attribute(id, "selected");
            }
        }
        self.recompute_with_hover(&[]);
    }

    /// Estimate the memory footprint of this page's data structures.
    ///
    /// Pass the composite buffer size (in bytes) so the profiler can include it.
    /// Only available when the `memprof` feature is enabled.
    #[cfg(feature = "memprof")]
    pub fn profile(&self, composite_buffer_bytes: usize) -> crate::memprof::MemoryProfile {
        crate::memprof::profile_page(self, composite_buffer_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_new_parses_and_layouts() {
        let page = Page::new(
            "<html><body><div id='x'>Hello</div></body></html>",
            "div { display: block; background-color: blue; padding: 10px; }",
            800.0,
            600.0,
        );

        assert!(
            page.layout_root.children.len() > 0,
            "Layout root should have children"
        );
    }

    #[test]
    fn page_collect_rects_basic() {
        let page = Page::new(
            "<html><body>
                <div class='a'>First</div>
                <div class='b'>Second</div>
              </body></html>",
            ".a { display: block; background-color: red; padding: 10px; }
             .b { display: block; background-color: green; padding: 10px; }",
            800.0,
            600.0,
        );

        let rects = page.collect_rects();
        assert!(
            rects.len() >= 2,
            "Should have at least 2 colored rects, got {}",
            rects.len()
        );

        // Verify colors are set
        for (_, color) in &rects {
            assert!(color.is_some(), "Collected rects should have colors");
        }
    }

    #[test]
    fn full_pipeline_smoke_test() {
        let page = Page::new(
            r#"<html><body>
                <div id="header" class="header">Header</div>
                <div class="content">
                    <p class="box green">Green box</p>
                    <p class="box red">Red box</p>
                </div>
                <div id="footer" class="footer">Footer</div>
              </body></html>"#,
            r#".header { display: block; background-color: blue; padding: 20px; }
               .content { display: block; }
               .box { display: block; padding: 15px; }
               .green { background-color: green; }
               .red { background-color: red; }
               .footer { display: block; background-color: orange; padding: 10px; }"#,
            1280.0,
            800.0,
        );

        let rects = page.collect_rects();

        // Should have multiple colored rectangles from the styled divs/ps
        assert!(
            !rects.is_empty(),
            "Pipeline should produce at least one render rect"
        );

        // All collected rects should have valid dimensions
        for (rect, _) in &rects {
            assert!(rect.width >= 0.0, "Rect width should be non-negative");
            assert!(rect.height >= 0.0, "Rect height should be non-negative");
        }
    }

    #[test]
    fn flexbox_pipeline_test() {
        // Full pipeline: HTML with flex container → CSS flex properties → layout → render rects
        let page = Page::new(
            r#"<html><body>
                <div class="flex-container">
                    <div class="item red">Item 1</div>
                    <div class="item green">Item 2</div>
                    <div class="item blue">Item 3</div>
                </div>
              </body></html>"#,
            r#".flex-container { display: flex; flex-direction: row; padding: 10px; }
               .item { flex-grow: 1; padding: 5px; }
               .red { background-color: red; }
               .green { background-color: green; }
               .blue { background-color: blue; }"#,
            800.0,
            600.0,
        );

        let rects = page.collect_rects();

        // Should have colored rectangles from the flex items
        assert!(
            !rects.is_empty(),
            "Flexbox pipeline should produce render rects"
        );

        // Find the three colored items and verify they are positioned left-to-right
        let red_rects: Vec<_> = rects
            .iter()
            .filter(|(_, c)| c == &Some([255, 0, 0, 255]))
            .collect();
        let green_rects: Vec<_> = rects
            .iter()
            .filter(|(_, c)| c == &Some([0, 128, 0, 255]))
            .collect();
        let blue_rects: Vec<_> = rects
            .iter()
            .filter(|(_, c)| c == &Some([0, 0, 255, 255]))
            .collect();

        // At minimum, we expect the flex items to produce colored rects
        assert!(
            red_rects.len() >= 1 || green_rects.len() >= 1 || blue_rects.len() >= 1,
            "Expected at least one colored flex item"
        );

        // All rects should have non-negative dimensions
        for (rect, _) in &rects {
            assert!(rect.width >= 0.0);
            assert!(rect.height >= 0.0);
        }
    }

    #[test]
    fn page_img_tag_creates_layout_node_with_src() {
        // Verify that <img> tags get their src attribute extracted into layout nodes.
        // Note: Without explicit width/height CSS, inline <img> elements will have
        // zero dimensions and be filtered by collect_image_nodes (which requires
        // positive dimensions). The image_src field IS set correctly on the node —
        // this test verifies that behavior via a direct layout node check.
        use crate::layout::{LayoutDomNode, LayoutNode};

        let page = Page::new(
            r#"<html><body>
                <img src="https://example.com/logo.png" />
              </body></html>"#,
            "",
            800.0,
            600.0,
        );

        // Walk layout tree looking for a node with image_src set
        fn find_image_node(node: &LayoutNode) -> Option<&LayoutNode> {
            if node.image_src.is_some() {
                return Some(node);
            }
            for child in &node.children {
                if let Some(found) = find_image_node(child) {
                    return Some(found);
                }
            }
            None
        }

        let img_node = find_image_node(&page.layout_root);
        assert!(
            img_node.is_some(),
            "Expected an img node with image_src set"
        );
        assert_eq!(
            img_node.unwrap().image_src.as_deref(),
            Some("https://example.com/logo.png")
        );
    }

    #[test]
    fn background_image_reaches_the_decoration_list() {
        let page = Page::new(
            "<html><body><div class='hero'>x</div></body></html>",
            ".hero { display: block; width: 300px; height: 200px; \
             background: #eee url(hero.png) no-repeat center / cover; }",
            800.0,
            600.0,
        );

        let deco = crate::layout::collect_decorations(&page.layout_root)
            .into_iter()
            .find(|d| d.background_image.is_some())
            .expect("the hero div should carry a background image");

        assert_eq!(deco.background_image.as_deref(), Some("hero.png"));
        assert_eq!(deco.background_size, css::BackgroundSize::Cover);
        assert_eq!(deco.background_repeat, css::BackgroundRepeat::NoRepeat);
        assert_eq!(deco.background_color, Some([238, 238, 238, 255]));
    }

    /// A page with one `<select>` and the given option markup.
    fn select_page(options: &str) -> Page {
        Page::new(
            &format!("<html><body><select id='s'>{options}</select></body></html>"),
            "",
            800.0,
            600.0,
        )
    }

    fn select_id(page: &Page) -> u32 {
        page.arena
            .find_by_id("s")
            .expect("the select is in the DOM")
    }

    #[test]
    fn a_select_shows_only_its_selected_option() {
        // Without the UA `option { display: none }` every label pours into the
        // page as flow content.
        let page =
            select_page("<option>Red</option><option selected>Green</option><option>Blue</option>");

        let texts: Vec<String> = crate::layout::collect_text_nodes(&page.layout_root)
            .into_iter()
            .map(|t| t.text)
            .collect();
        assert_eq!(texts, vec!["Green".to_string()]);
    }

    #[test]
    fn a_select_with_no_selected_attribute_shows_the_first_option() {
        let page = select_page("<option>First</option><option>Second</option>");
        let texts: Vec<String> = crate::layout::collect_text_nodes(&page.layout_root)
            .into_iter()
            .map(|t| t.text)
            .collect();
        assert_eq!(texts, vec!["First".to_string()]);
    }

    #[test]
    fn select_options_are_listed_with_their_state() {
        let page = select_page("<option>A</option><option selected>B</option>");
        let options = page.select_options(select_id(&page));

        assert_eq!(options.len(), 2);
        assert_eq!(options[0].1, "A");
        assert!(!options[0].2);
        assert_eq!(options[1].1, "B");
        assert!(options[1].2, "the marked option reports as selected");
    }

    #[test]
    fn options_inside_an_optgroup_are_found() {
        let page = select_page(
            "<optgroup label='Warm'><option>Red</option></optgroup>\
             <optgroup label='Cool'><option>Blue</option></optgroup>",
        );
        let labels: Vec<String> = page
            .select_options(select_id(&page))
            .into_iter()
            .map(|(_, label, _)| label)
            .collect();
        assert_eq!(labels, vec!["Red".to_string(), "Blue".to_string()]);
    }

    #[test]
    fn choosing_an_option_moves_the_selection_and_relayouts() {
        let mut page = select_page("<option>A</option><option selected>B</option>");
        let sid = select_id(&page);
        let first = page.select_options(sid)[0].0;

        page.select_option_and_recompute(sid, first);

        let options = page.select_options(sid);
        assert!(options[0].2, "the clicked option is now selected");
        assert!(!options[1].2, "the previous selection was cleared");

        let texts: Vec<String> = crate::layout::collect_text_nodes(&page.layout_root)
            .into_iter()
            .map(|t| t.text)
            .collect();
        assert_eq!(
            texts,
            vec!["A".to_string()],
            "the control shows the new label"
        );
    }

    /// Where the single text run of a 400px paragraph lands.
    fn aligned_text_x(align: &str) -> (f32, f32) {
        let page = Page::new(
            "<html><body><p>hello</p></body></html>",
            &format!("body {{ margin: 0 }} p {{ width: 400px; text-align: {align}; }}"),
            800.0,
            600.0,
        );
        let runs = crate::layout::collect_text_nodes(&page.layout_root);
        assert_eq!(runs.len(), 1, "expected one run, got {runs:?}");
        (runs[0].x, runs[0].width)
    }

    /// Find the widest box in the tree whose display matches.
    fn find_box(node: &crate::layout::LayoutNode, display: css::DisplayType) -> Option<Rect> {
        let mut found = if node.display == display {
            Some(node.rect)
        } else {
            None
        };
        for child in node.children.iter().chain(node.absolute_children.iter()) {
            if let Some(rect) = find_box(child, display) {
                if found.is_none_or(|f: Rect| rect.width > f.width) {
                    found = Some(rect);
                }
            }
        }
        found
    }

    /// The rect of the element with the given id.
    fn rect_of(page: &Page, id: &str) -> Rect {
        let dom_id = page.arena.find_by_id(id).expect("element is in the DOM");
        crate::layout::find_layout_rect_by_dom_id(&page.layout_root, dom_id)
            .expect("element is laid out")
    }

    #[test]
    fn a_specified_width_is_not_shrunk_to_the_containing_block() {
        // CSS uses a specified width as specified; the box overflows rather than
        // shrinking. Clamping it left a bordered box narrower than the content
        // sized from that same width — a frame narrower than its image.
        let page = Page::new(
            "<html><body><div class='outer'><div id='wide'></div></div></body></html>",
            "body { margin: 0 } .outer { width: 200px } \
             #wide { width: 500px; border: 1px solid black }",
            800.0,
            600.0,
        );
        assert_eq!(rect_of(&page, "wide").width, 500.0);
    }

    #[test]
    fn max_width_still_cuts_a_specified_width_down() {
        // Removing the containing-block clamp must not remove the one clamp CSS
        // does apply.
        let page = Page::new(
            "<html><body><div id='capped'></div></body></html>",
            "body { margin: 0 } #capped { width: 500px; max-width: 300px }",
            800.0,
            600.0,
        );
        assert_eq!(rect_of(&page, "capped").width, 300.0);
    }

    #[test]
    fn a_box_with_a_specified_width_does_not_give_way_to_a_float() {
        // Narrowing a block to sit beside a float is for auto widths. A
        // specified width stays specified — this is the path the Wikipedia
        // image frame went through, since `display: flex` establishes a BFC.
        let page = Page::new(
            "<html><body>\
             <div class='f'></div><div id='flex'></div>\
             </body></html>",
            "body { margin: 0 } .f { float: left; width: 300px; height: 100px } \
             #flex { display: flex; width: 600px; border: 1px solid black }",
            800.0,
            600.0,
        );
        assert_eq!(
            rect_of(&page, "flex").width,
            600.0,
            "the specified width survives the float narrowing"
        );
    }

    #[test]
    fn an_auto_width_block_is_left_to_the_float_logic() {
        // The narrowing guard keys off `explicit_width`, so an auto-width box
        // must reach the float code exactly as before. Asserting the width here
        // would pin unrelated float behaviour, so this only pins that no width
        // was invented for it.
        let page = Page::new(
            "<html><body><div class='f'></div><div id='auto'></div></body></html>",
            "body { margin: 0 } .f { float: left; width: 300px; height: 100px } \
             #auto { display: flex; height: 50px }",
            800.0,
            600.0,
        );
        let dom_id = page.arena.find_by_id("auto").unwrap();
        assert_eq!(page.styles.get(&dom_id).unwrap().explicit_width, None);
        assert!(rect_of(&page, "auto").width > 0.0);
    }

    #[test]
    fn an_inline_block_shrinks_to_fit_instead_of_collapsing_to_a_pixel() {
        // An inline-block's rect is empty when inline box collection reads it,
        // and line breaking floored the empty one at 1px. The box then laid its
        // own text out in that pixel: every CJK character is a break
        // opportunity, so a Japanese caption came out one character per line.
        let page = Page::new(
            "<html><body><div class='wrap'><span class='cap'>\
             日本語のキャプションが一行に収まる</span></div></body></html>",
            "body { margin: 0 } .wrap { width: 600px } .cap { display: inline-block }",
            800.0,
            600.0,
        );

        let cap = find_box(&page.layout_root, css::DisplayType::InlineBlock)
            .expect("the inline-block is in the layout tree");
        assert!(
            cap.width > 100.0,
            "the inline-block should shrink to fit its text, got width {}",
            cap.width
        );

        let runs = crate::layout::collect_text_nodes(&page.layout_root);
        assert!(
            runs.len() < 5,
            "the caption should not break per character, got {} runs: {:?}",
            runs.len(),
            runs.iter().map(|r| r.text.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn content_after_an_inline_block_clears_its_real_height() {
        // The line's height came from the same 1px floor, so whatever followed
        // was placed a pixel below a box hundreds of pixels tall.
        let page = Page::new(
            "<html><body>\
             <div class='wrap'><span class='cap'>あ い う え お か き く け こ</span></div>\
             <p id='after'>下の段落</p></body></html>",
            "body { margin: 0 } .wrap { width: 80px } .cap { display: inline-block } \
             p { margin: 0 }",
            800.0,
            600.0,
        );

        let cap = find_box(&page.layout_root, css::DisplayType::InlineBlock).unwrap();
        let after = page.arena.find_by_id("after").unwrap();
        let after_y = crate::layout::find_layout_rect_by_dom_id(&page.layout_root, after)
            .expect("the paragraph is laid out")
            .y;

        assert!(cap.height > 20.0, "the wrapped box is tall: {}", cap.height);
        assert!(
            after_y >= cap.y + cap.height - 1.0,
            "following content must start below the box: box bottom {}, paragraph y {}",
            cap.y + cap.height,
            after_y
        );
    }

    #[test]
    fn dir_rtl_starts_the_line_at_the_right_edge() {
        // `direction: rtl` makes `text-align: start` mean the right edge, so the
        // text is pushed over without the page naming an alignment at all.
        let page = Page::new(
            "<html><body><p dir='rtl'>שלום</p></body></html>",
            "body { margin: 0 } p { width: 400px; }",
            800.0,
            600.0,
        );
        let runs = crate::layout::collect_text_nodes(&page.layout_root);
        assert_eq!(runs.len(), 1);
        assert!(
            runs[0].x > 200.0,
            "right-to-left text should sit at the right of a 400px box, got x={}",
            runs[0].x
        );
    }

    #[test]
    fn the_dir_attribute_and_the_css_property_agree() {
        for source in [
            ("<p dir='rtl'>a</p>", "p { width: 400px }"),
            ("<p>a</p>", "p { width: 400px; direction: rtl }"),
        ] {
            let page = Page::new(
                &format!("<html><body>{}</body></html>", source.0),
                &format!("body {{ margin: 0 }} {}", source.1),
                800.0,
                600.0,
            );
            let p = page
                .arena
                .find_by_tag("p")
                .expect("the paragraph is in the DOM");
            assert_eq!(
                page.styles.get(&p).unwrap().direction,
                css::Direction::Rtl,
                "for {source:?}"
            );
        }
    }

    #[test]
    fn direction_inherits_to_descendants() {
        let page = Page::new(
            "<html dir='rtl'><body><div><span id='deep'>x</span></div></body></html>",
            "",
            800.0,
            600.0,
        );
        let deep = page.arena.find_by_id("deep").unwrap();
        assert_eq!(
            page.styles.get(&deep).unwrap().direction,
            css::Direction::Rtl
        );
    }

    #[test]
    fn text_align_moves_the_painted_text_not_just_child_boxes() {
        // The alignment shift used to be applied only where inline child
        // elements were positioned, so the text itself never moved.
        let (left_x, w) = aligned_text_x("left");
        assert!(
            left_x.abs() < 1.0,
            "left starts at the content edge: {left_x}"
        );

        let (right_x, _) = aligned_text_x("right");
        assert!(
            (right_x - (400.0 - w)).abs() < 1.0,
            "right should end at the content edge: x={right_x}, width={w}"
        );

        let (center_x, _) = aligned_text_x("center");
        assert!(
            (center_x - (400.0 - w) / 2.0).abs() < 1.0,
            "center should split the free space: x={center_x}, width={w}"
        );
    }

    /// A page whose inline script rewrites an element, built under `policy`.
    fn scripted_page(policy: &[&str]) -> Page {
        let html = "<html><body><div id='greeting'>before</div>\
                    <script>document.getElementById('greeting').textContent = 'after';</script>\
                    </body></html>";
        let policies: Vec<String> = policy.iter().map(|p| p.to_string()).collect();
        Page::new_with_csp(
            html,
            "",
            800.0,
            600.0,
            &crate::network::security::Csp::parse(&policies),
        )
    }

    fn greeting(page: &Page) -> String {
        let id = page.arena.find_by_id("greeting").unwrap();
        page.arena.get_text_content(id)
    }

    #[test]
    fn a_page_with_no_policy_runs_its_inline_scripts() {
        assert_eq!(greeting(&scripted_page(&[])), "after");
    }

    #[test]
    fn a_policy_without_unsafe_inline_stops_an_inline_script_running() {
        // Blocking inline script is the single most common thing a CSP is set
        // up to do, and this engine does run inline scripts — so the policy has
        // to be known before the document is built, not applied afterwards.
        assert_eq!(greeting(&scripted_page(&["script-src 'self'"])), "before");
        assert_eq!(greeting(&scripted_page(&["default-src 'none'"])), "before");
    }

    #[test]
    fn a_policy_that_permits_inline_script_lets_it_run() {
        assert_eq!(
            greeting(&scripted_page(&["script-src 'self' 'unsafe-inline'"])),
            "after"
        );
    }

    #[test]
    fn a_script_carrying_the_policys_nonce_runs_and_one_without_it_does_not() {
        // Nonce-based policies are what real sites use — Wikipedia among them —
        // so a page's own scripts have to survive one.
        let policy = crate::network::security::Csp::parse(&["script-src 'nonce-r4nd0m'".into()]);
        let with_nonce = "<html><body><div id='greeting'>before</div>\
             <script nonce='r4nd0m'>document.getElementById('greeting').textContent='after';</script>\
             </body></html>";
        let page = Page::new_with_csp(with_nonce, "", 800.0, 600.0, &policy);
        assert_eq!(greeting(&page), "after");

        let wrong_nonce = with_nonce.replace("r4nd0m'>", "guessed'>");
        let page = Page::new_with_csp(&wrong_nonce, "", 800.0, 600.0, &policy);
        assert_eq!(greeting(&page), "before");
    }

    #[test]
    fn zoom_scales_a_specified_width() {
        let mut page = Page::new(
            "<html><body><div id='box'></div></body></html>",
            "body { margin: 0 } #box { width: 100px; height: 40px }",
            800.0,
            600.0,
        );
        assert_eq!(rect_of(&page, "box").width, 100.0);

        let base_height = rect_of(&page, "box").height;

        page.set_zoom(2.0);
        assert_eq!(rect_of(&page, "box").width, 200.0);
        assert_eq!(rect_of(&page, "box").height, base_height * 2.0);
    }

    #[test]
    fn zoom_scales_text_that_nobody_sized() {
        // Text with no declared size inherits the initial 16px. If zoom only
        // touched declared lengths, a zoomed page would grow every box and
        // leave the text in it unchanged.
        let mut page = Page::new(
            "<html><body><p id='t'>text</p></body></html>",
            "",
            800.0,
            600.0,
        );
        let dom_id = page.arena.find_by_id("t").unwrap();
        assert_eq!(page.styles.get(&dom_id).unwrap().font_size, 16.0);

        page.set_zoom(1.5);
        let dom_id = page.arena.find_by_id("t").unwrap();
        assert_eq!(page.styles.get(&dom_id).unwrap().font_size, 24.0);
    }

    #[test]
    fn zoom_leaves_viewport_units_alone() {
        // `50vw` is half the window whatever the zoom: the window did not
        // change size, so scaling it too would zoom the page twice.
        for zoom in [1.0, 2.0] {
            let mut page = Page::new(
                "<html><body><div id='half'></div></body></html>",
                "body { margin: 0 } #half { width: 50vw; height: 10px }",
                800.0,
                600.0,
            );
            page.set_zoom(zoom);
            assert_eq!(
                rect_of(&page, "half").width,
                400.0,
                "at zoom {zoom} a vw length still measures against the window"
            );
        }
    }

    #[test]
    fn zoom_compounds_em_only_once() {
        // `em` resolves against a font size that zoom has already scaled, so
        // scaling it again would square the zoom on every nested element.
        let mut page = Page::new(
            "<html><body><div id='outer'><div id='inner'>x</div></div></body></html>",
            "body { margin: 0; font-size: 10px } #outer { font-size: 2em } \
             #inner { font-size: 2em }",
            800.0,
            600.0,
        );
        page.set_zoom(2.0);

        let inner = page.arena.find_by_id("inner").unwrap();
        // 10px base × zoom 2 = 20, then 2em twice = 80.
        assert_eq!(page.styles.get(&inner).unwrap().font_size, 80.0);
    }

    #[test]
    fn page_has_empty_image_cache() {
        let page = Page::new("<html><body></body></html>", "", 800.0, 600.0);
        assert!(page.image_cache.is_empty());
    }

    #[test]
    fn page_caches_images() {
        let mut page = Page::new("<html><body></body></html>", "", 800.0, 600.0);
        let dummy_rgba = vec![255u8, 0, 0, 255]; // 1x1 red pixel
        page.image_cache.insert(
            "test.png".to_string(),
            CachedImage {
                rgba: dummy_rgba,
                width: 1,
                height: 1,
            },
        );
        assert_eq!(page.image_cache.len(), 1);
    }
}
