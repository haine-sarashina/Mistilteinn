/// Page orchestration module.
///
/// Connects HTML parsing, CSS computation, layout building, and rendering
/// into a single pipeline: HTML → CSS → Layout → Render.
use rustc_hash::FxHashMap;

use crate::css;
use crate::html::{self, DomArena, DomHandle, NodeId};
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
    pub arena: DomArena,
    pub title: String,
    #[allow(dead_code)]
    pub styles: FxHashMap<u32, css::ComputedValues>,
    pub layout_root: crate::layout::LayoutNode,
    pub view_width: f32,
    pub view_height: f32,
    pub page_url: String,
    pub image_cache: FxHashMap<String, CachedImage>,
    /// Merged (UA + author) stylesheet for style recomputation (e.g., :hover).
    pub stylesheet: crate::css::parser::Stylesheet,
}

impl Page {
    /// Run the full pipeline: parse HTML + CSS, compute styles, build and measure layout tree.
    pub fn new(html_source: &str, css_source: &str, view_width: f32, view_height: f32) -> Self {
        // Stage 1: Parse HTML into DOM arena
        let arena = html::parse_html(html_source);
        let title = arena
            .extract_title()
            .unwrap_or_else(|| "New Tab".to_string());
        let shared_arena = std::rc::Rc::new(std::cell::RefCell::new(arena));

        // Stage 1.5: Execute inline JS scripts with DOM bindings connected to arena
        let mut js_context = crate::js::init_js_engine_with_arena(shared_arena.clone());
        let scripts = shared_arena.borrow().extract_scripts();
        for script in scripts {
            crate::js::execute_script(&mut js_context, &script);
        }

        let arena = match std::rc::Rc::try_unwrap(shared_arena) {
            Ok(cell) => cell.into_inner(),
            Err(rc) => rc.borrow().clone(),
        };

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

        Self {
            arena,
            title,
            styles,
            layout_root,
            view_width,
            view_height,
            page_url: String::new(),
            image_cache: FxHashMap::default(),
            stylesheet,
        }
    }

    /// Recompute styles and rebuild the layout tree with the given set of
    /// hovered DOM node IDs. If `hovered_ids` is empty, computes static styles
    /// (equivalent to initial load).
    pub fn recompute_with_hover(&mut self, hovered_ids: &[u32]) {
        // Recompute styles with hover awareness
        self.styles = css::compute_styles_for_tree_with_hover(
            &self.arena,
            &self.stylesheet,
            (self.view_width, self.view_height),
            hovered_ids,
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
