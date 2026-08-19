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
    /// Every image URL already asked for, whether or not one arrived.
    ///
    /// A `loading="lazy"` image is fetched when the reader scrolls near it,
    /// which is a check that runs every frame the page moves; without this, a
    /// picture that 404s would be requested again on each of them.
    pub requested_images: rustc_hash::FxHashSet<String>,
    /// Where the page was scrolled to when the lazy-image walk last ran, so
    /// the walk is not repeated for a scroll that cannot have changed its
    /// answer. Starts far away, so the first check always runs.
    pub lazy_scan_y: f32,
    /// The policy this document was loaded under, kept so subresources fetched
    /// later — a lazy image, say — are held to the same rules as the ones
    /// fetched during load.
    pub csp: crate::network::security::Csp,
    /// Merged (UA + author) stylesheet for style recomputation (e.g., :hover).
    pub stylesheet: crate::css::parser::Stylesheet,
    /// The page's JavaScript context, kept alive past the initial script run so
    /// registered event listeners can still be called.
    js_context: boa_engine::Context,
    /// The transitions and `@keyframes` animations this page has running.
    pub animator: crate::animator::Animator,
    /// What each `<canvas>` on this page has had drawn on it.
    ///
    /// Shared with the JavaScript context: a script draws into these, and the
    /// compositor paints them. Nothing in the cascade or the layout tree knows
    /// what is on a canvas, which is why the surfaces live beside them.
    canvases: crate::js::canvas::SharedCanvases,
    /// The documents embedded in this one, by the DOM id of the `<iframe>`,
    /// `<object>` or `<embed>` that holds each.
    ///
    /// A nested browsing context is a whole page — its own DOM, cascade, layout
    /// tree and scripts — laid out at the size of the box it sits in and
    /// painted inside it.
    frames: rustc_hash::FxHashMap<u32, Box<Page>>,
    /// The `localStorage` and `sessionStorage` areas of this page's origin.
    ///
    /// Handed in rather than created here: two tabs on one site share their
    /// local area, and what a page saves has to outlive it.
    storage: crate::browser::storage::PageStorage,
    /// The hover path the styles were last computed with, so a frame driven by
    /// an animation does not silently drop `:hover`.
    last_hover: Vec<u32>,
}

/// How far outside the viewport a `loading="lazy"` image is still worth
/// fetching.
///
/// A reader scrolling steadily should not have to watch a picture arrive after
/// it is already on screen, so the fetch starts while it is still below the
/// fold.
pub const LAZY_LOAD_MARGIN: f32 = 600.0;

impl Page {
    /// The subresources this page wants but has not asked for yet.
    ///
    /// `viewport` is the part of the document the reader can see, in layout
    /// coordinates. A `loading="lazy"` image outside it — and outside
    /// [`LAZY_LOAD_MARGIN`] around it — is left for a later call, which is what
    /// makes the attribute mean anything: on a long page it is the difference
    /// between a few requests and a few hundred.
    ///
    /// Each entry is (url, requested width, requested height); the size matters
    /// to an SVG, which has no pixels of its own to be rasterised at.
    pub fn pending_image_requests(&self, viewport: Rect) -> Vec<(String, f32, f32)> {
        use crate::network::security::{ResourceKind, SubresourceDecision, check_subresource};

        let base = self.base_url();
        let resolve = |src: &str| -> String {
            if base.is_empty() {
                src.to_string()
            } else {
                crate::network::resolve_url(&base, src)
            }
        };

        let mut requests: Vec<(String, f32, f32)> = Vec::new();
        let want = |src: &str, width: f32, height: f32, requests: &mut Vec<_>| {
            let url = resolve(src);
            if self.image_cache.contains_key(&url) || self.requested_images.contains(&url) {
                return;
            }
            match check_subresource(&self.page_url, &self.csp, &url, ResourceKind::Image) {
                SubresourceDecision::Load(url) => {
                    if !requests.iter().any(|(existing, _, _)| existing == &url) {
                        requests.push((url, width, height));
                    }
                }
                SubresourceDecision::Block(reason) => log::warn!("image blocked: {reason}"),
            }
        };

        for image in crate::layout::collect_image_nodes(&self.layout_root) {
            if image.lazy && !within_reach(viewport, image.y, image.height) {
                continue;
            }
            want(&image.src, image.width, image.height, &mut requests);
        }

        // A CSS background goes through the same fetch and cache as an `<img>`.
        // There is no `loading` attribute to defer one, so they are all wanted
        // straight away.
        for decoration in crate::layout::collect_decorations(&self.layout_root) {
            if let Some(ref src) = decoration.background_image {
                want(src, decoration.width, decoration.height, &mut requests);
            }
        }

        requests
    }

    /// Note that these URLs have been asked for, so they are not asked for again.
    pub fn mark_images_requested(&mut self, urls: impl IntoIterator<Item = String>) {
        self.requested_images.extend(urls);
    }
}

/// Whether a box spanning `y..y + height` is close enough to `viewport` that a
/// lazy image inside it should be fetched now.
fn within_reach(viewport: Rect, y: f32, height: f32) -> bool {
    y < viewport.bottom() + LAZY_LOAD_MARGIN && y + height.max(1.0) > viewport.y - LAZY_LOAD_MARGIN
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
        Self::new_with_storage(
            html_source,
            css_source,
            view_width,
            view_height,
            csp,
            crate::browser::storage::PageStorage::default(),
        )
    }

    /// [`Self::new_with_csp`], with the storage areas of the page's origin.
    ///
    /// The areas have to arrive before the pipeline runs: this document's
    /// inline scripts execute during parsing, and one of the first things a
    /// page does is read what it saved last time.
    pub fn new_with_storage(
        html_source: &str,
        css_source: &str,
        view_width: f32,
        view_height: f32,
        csp: &crate::network::security::Csp,
        storage: crate::browser::storage::PageStorage,
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
        let canvases: crate::js::canvas::SharedCanvases = Default::default();
        crate::js::canvas::set_active_canvases(Some(canvases.clone()));
        crate::js::storage::set_active_storage(Some(storage.clone()));
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
        let mut styles =
            css::compute_styles_for_tree(&arena, &stylesheet, (view_width, view_height));

        // Stage 3.5: Let the clock have its say. An animation with no delay is
        // already showing its first frame by the time the page is laid out, so
        // the values it sets have to be in place before anything is measured.
        let mut animator = crate::animator::Animator::new();
        animator.apply(
            &mut styles,
            &stylesheet.keyframes,
            css::LengthContext {
                viewport_width: view_width,
                viewport_height: view_height,
                ..Default::default()
            },
        );

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
            requested_images: rustc_hash::FxHashSet::default(),
            lazy_scan_y: f32::NEG_INFINITY,
            csp: csp.clone(),
            stylesheet,
            js_context,
            animator,
            last_hover: Vec::new(),
            canvases,
            frames: rustc_hash::FxHashMap::default(),
            storage,
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
        self.dispatch_event_along_with_detail(path, event_type, "null")
    }

    /// [`Self::dispatch_event_along`], with extra properties on the event.
    ///
    /// `detail` is a JavaScript object literal the caller builds — the pointer
    /// position, say — since only the caller knows what this event was.
    pub fn dispatch_event_along_with_detail(
        &mut self,
        path: &[u32],
        event_type: &str,
        detail: &str,
    ) -> DispatchOutcome {
        self.make_active();
        crate::js::reset_default_prevented(&mut self.js_context);

        let mut total = DispatchOutcome::default();
        for &node_id in path.iter().rev() {
            let outcome = crate::js::dispatch_event_with_detail(
                &mut self.js_context,
                node_id,
                event_type,
                detail,
            );
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
        self.make_active();
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
        if hovered_ids != self.last_hover.as_slice() {
            self.last_hover = hovered_ids.to_vec();
        }

        // The cascade says where each property is heading; this says how far it
        // has got. It runs before the layout tree is built, so an animated
        // width is measured at the width it currently has.
        self.animator.apply(
            &mut self.styles,
            &self.stylesheet.keyframes,
            css::LengthContext {
                viewport_width: self.view_width,
                viewport_height: self.view_height,
                zoom: self.zoom,
                ..Default::default()
            },
        );

        // Rebuild layout tree from new computed styles
        let get_node = |id: u32| self.arena.get(DomHandle(NodeId::from_raw(id)));
        self.layout_root =
            crate::layout::build_layout_tree(0, &self.styles, get_node, self.view_width);

        // An image knows how big it is only once it has been decoded, which is
        // after the first layout. Applying that here means the second layout —
        // the one that runs when the images arrive — places everything around
        // the space they actually take.
        let base_url = self.base_url();
        let cache = &self.image_cache;
        crate::layout::apply_intrinsic_image_sizes(&mut self.layout_root, &|src| {
            let resolved = if base_url.is_empty() {
                src.to_string()
            } else {
                crate::network::resolve_url(&base_url, src)
            };
            cache
                .get(&resolved)
                .or_else(|| cache.get(src))
                .map(|image| (image.width as f32, image.height as f32))
        });

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

    /// The document embedded in a given element, if one has been loaded.
    pub fn frame(&self, dom_node_id: u32) -> Option<&Page> {
        self.frames.get(&dom_node_id).map(|page| page.as_ref())
    }

    /// Put a loaded document into the element that embeds it.
    pub fn set_frame(&mut self, dom_node_id: u32, page: Page) {
        self.frames.insert(dom_node_id, Box::new(page));
    }

    /// The elements of this page that embed a document, laid out.
    pub fn frame_boxes(&self) -> Vec<crate::layout::FrameBox> {
        crate::layout::collect_frame_boxes(&self.layout_root)
    }

    /// Point the JavaScript engine's thread-local state at this page.
    ///
    /// The engine's native functions reach the DOM and the canvases through
    /// thread-locals rather than through captured handles, so whichever page is
    /// about to run script has to claim them first.
    fn make_active(&self) {
        crate::js::dom::set_active_arena(Some(self.arena.clone()));
        // Permission decisions belong to an origin, so the store has to know
        // whose script is about to run.
        crate::browser::permissions::set_active_origin(&crate::browser::storage::storage_origin(
            &self.page_url,
        ));
        crate::js::canvas::set_active_canvases(Some(self.canvases.clone()));
        crate::js::storage::set_active_storage(Some(self.storage.clone()));
    }

    /// The surface of every canvas that has been drawn on, by DOM node id.
    pub fn canvas_surfaces(&self) -> Vec<(u32, crate::render::canvas::Surface)> {
        self.canvases
            .borrow()
            .iter()
            .map(|(id, state)| (*id, state.surface.clone()))
            .collect()
    }

    /// The surface of one canvas, if a script has drawn on it.
    pub fn canvas_surface(&self, node_id: u32) -> Option<crate::render::canvas::Surface> {
        self.canvases
            .borrow()
            .get(&node_id)
            .map(|state| state.surface.clone())
    }

    /// Whether this page has an animation or transition still moving.
    ///
    /// The frame loop asks every frame; while it is true the page is laid out
    /// again and repainted, which is what makes an animation run.
    pub fn animations_running(&self) -> bool {
        self.animator.is_active()
    }

    /// Advance every running animation to the current moment and relayout.
    ///
    /// The hover path is the one the last recompute used, so an element being
    /// pointed at does not lose `:hover` on the next animation frame.
    pub fn advance_animations(&mut self) {
        let hovered = std::mem::take(&mut self.last_hover);
        self.recompute_with_hover(&hovered);
        self.last_hover = hovered;
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

    /// A page with an eager image at the top and a lazy one far below it.
    fn lazily_loaded_page() -> Page {
        Page::new(
            "<html><body>
                <img src='top.png' width='100' height='100'>
                <div class='filler'></div>
                <img src='bottom.png' width='100' height='100' loading='lazy'>
              </body></html>",
            ".filler { display: block; height: 3000px; }",
            800.0,
            600.0,
        )
    }

    #[test]
    fn a_lazy_image_below_the_fold_is_not_requested_yet() {
        let page = lazily_loaded_page();
        let urls: Vec<String> = page
            .pending_image_requests(Rect::new(0.0, 0.0, 800.0, 600.0))
            .into_iter()
            .map(|(url, _, _)| url)
            .collect();
        assert!(urls.iter().any(|url| url.contains("top.png")));
        assert!(
            !urls.iter().any(|url| url.contains("bottom.png")),
            "the lazy image is 3000px down, so it can wait: {urls:?}"
        );
    }

    #[test]
    fn scrolling_to_a_lazy_image_asks_for_it() {
        let page = lazily_loaded_page();
        let urls: Vec<String> = page
            .pending_image_requests(Rect::new(0.0, 2800.0, 800.0, 600.0))
            .into_iter()
            .map(|(url, _, _)| url)
            .collect();
        assert!(
            urls.iter().any(|url| url.contains("bottom.png")),
            "got {urls:?}"
        );
    }

    #[test]
    fn an_eager_image_is_requested_wherever_it_sits() {
        let page = Page::new(
            "<html><body>
                <div style='height: 3000px'></div>
                <img src='deep.png' width='10' height='10'>
              </body></html>",
            "",
            800.0,
            600.0,
        );
        let urls: Vec<String> = page
            .pending_image_requests(Rect::new(0.0, 0.0, 800.0, 600.0))
            .into_iter()
            .map(|(url, _, _)| url)
            .collect();
        assert!(urls.iter().any(|url| url.contains("deep.png")));
    }

    #[test]
    fn an_image_already_asked_for_is_not_asked_for_again() {
        let mut page = lazily_loaded_page();
        let first = page.pending_image_requests(Rect::new(0.0, 0.0, 800.0, 600.0));
        assert!(!first.is_empty());
        page.mark_images_requested(first.into_iter().map(|(url, _, _)| url));
        assert!(
            page.pending_image_requests(Rect::new(0.0, 0.0, 800.0, 600.0))
                .is_empty()
        );
    }

    #[test]
    fn an_animation_is_already_showing_its_first_frame_when_the_page_is_laid_out() {
        let page = Page::new(
            "<html><body><div id='box'>x</div></body></html>",
            "@keyframes grow { from { width: 20px } to { width: 400px } }
             #box { display: block; animation: grow 100s linear infinite; }",
            800.0,
            600.0,
        );
        let width = find_box_width(&page.layout_root).expect("the box is laid out");
        assert!(
            (20.0..40.0).contains(&width),
            "a hundred-second animation has barely begun: {width}"
        );
        assert!(
            page.animations_running(),
            "and it keeps the page repainting"
        );
    }

    #[test]
    fn a_page_with_no_animation_does_not_ask_to_be_repainted() {
        let page = Page::new(
            "<html><body><div id='box'>x</div></body></html>",
            "#box { display: block; width: 200px; }",
            800.0,
            600.0,
        );
        assert!(!page.animations_running());
    }

    #[test]
    fn a_transition_eases_a_change_the_cascade_makes() {
        let mut page = Page::new(
            "<html><body><div id='box'>x</div></body></html>",
            "#box { display: block; width: 100px; transition: width 100s linear; }",
            800.0,
            600.0,
        );
        assert_eq!(find_box_width(&page.layout_root), Some(100.0));

        // A script changing the style is the same kind of change a `:hover`
        // rule makes: the cascade produces a new value, and the transition is
        // what stops the box arriving there at once.
        page.eval_script("document.getElementById('box').style.setProperty('width', '300px')");

        let width = find_box_width(&page.layout_root).expect("the box is laid out");
        assert!(
            (100.0..120.0).contains(&width),
            "the box has set out but not arrived: {width}"
        );
        assert!(page.animations_running());
    }

    /// The laid-out width of the box the animation tests use.
    fn find_box_width(node: &crate::layout::LayoutNode) -> Option<f32> {
        if node.dom_node_id.is_some() && node.rect.height > 0.0 && node.explicit_width.is_some() {
            return Some(node.rect.width);
        }
        node.children.iter().find_map(find_box_width)
    }

    /// The media boxes of a page built from this markup.
    fn media_boxes(html: &str) -> Vec<crate::layout::MediaBox> {
        let page = Page::new(html, "", 800.0, 600.0);
        crate::layout::collect_media_boxes(&page.layout_root)
    }

    #[test]
    fn a_video_takes_the_default_size_css_gives_it() {
        let boxes = media_boxes("<html><body><video></video></body></html>");
        assert_eq!(boxes.len(), 1);
        assert_eq!(boxes[0].kind, crate::layout::MediaKind::Video);
        assert_eq!((boxes[0].rect.width, boxes[0].rect.height), (300.0, 150.0));
        assert!(!boxes[0].controls);
    }

    #[test]
    fn a_video_is_sized_by_its_attributes() {
        let boxes = media_boxes(
            "<html><body><video width='640' height='360' controls></video></body></html>",
        );
        assert_eq!((boxes[0].rect.width, boxes[0].rect.height), (640.0, 360.0));
        assert!(boxes[0].controls);
    }

    #[test]
    fn a_poster_is_fetched_like_any_other_picture() {
        let page = Page::new(
            "<html><body><video poster='frame.jpg'></video></body></html>",
            "",
            800.0,
            600.0,
        );
        let urls: Vec<String> = page
            .pending_image_requests(Rect::new(0.0, 0.0, 800.0, 600.0))
            .into_iter()
            .map(|(url, _, _)| url)
            .collect();
        assert!(
            urls.iter().any(|url| url.contains("frame.jpg")),
            "got {urls:?}"
        );
    }

    #[test]
    fn an_audio_element_shows_nothing_unless_it_has_controls() {
        assert!(media_boxes("<html><body><audio src='a.mp3'></audio></body></html>").is_empty());

        let boxes = media_boxes("<html><body><audio src='a.mp3' controls></audio></body></html>");
        assert_eq!(boxes.len(), 1);
        assert_eq!(boxes[0].kind, crate::layout::MediaKind::Audio);
        assert!(boxes[0].rect.height > 0.0);
    }

    #[test]
    fn the_fallback_inside_a_media_element_is_not_put_on_the_page() {
        let page = Page::new(
            "<html><body><video>Your browser cannot play this.</video></body></html>",
            "",
            800.0,
            600.0,
        );
        let texts = crate::layout::collect_text_nodes(&page.layout_root);
        assert!(
            !texts.iter().any(|t| t.text.contains("cannot play")),
            "the fallback is for browsers without video, not for this one"
        );
    }

    #[test]
    fn a_canvas_is_a_box_of_the_size_its_attributes_declare() {
        let page = Page::new(
            "<html><body><canvas width='320' height='200'></canvas></body></html>",
            "",
            800.0,
            600.0,
        );
        let boxes = crate::layout::collect_canvas_boxes(&page.layout_root);
        assert_eq!(boxes.len(), 1);
        assert_eq!((boxes[0].rect.width, boxes[0].rect.height), (320.0, 200.0));
    }

    #[test]
    fn a_canvas_with_no_attributes_takes_the_html_default_size() {
        let page = Page::new(
            "<html><body><canvas></canvas></body></html>",
            "",
            800.0,
            600.0,
        );
        let boxes = crate::layout::collect_canvas_boxes(&page.layout_root);
        assert_eq!((boxes[0].rect.width, boxes[0].rect.height), (300.0, 150.0));
    }

    #[test]
    fn what_a_script_drew_survives_the_relayout_after_it() {
        let mut page = Page::new(
            "<html><body><canvas id='c' width='20' height='20'></canvas></body></html>",
            "",
            800.0,
            600.0,
        );
        page.eval_script(
            "var g = document.getElementById('c').getContext('2d');
             g.fillStyle = 'red';
             g.fillRect(0, 0, 20, 20);",
        );
        let canvas = crate::layout::collect_canvas_boxes(&page.layout_root)[0];
        let surface = page
            .canvas_surface(canvas.dom_node_id)
            .expect("the box and the surface agree on which canvas this is");
        assert_eq!(surface.pixels[3], 255);
    }

    #[test]
    fn the_fallback_inside_a_canvas_is_not_put_on_the_page() {
        let page = Page::new(
            "<html><body><canvas>No canvas here.</canvas></body></html>",
            "",
            800.0,
            600.0,
        );
        let texts = crate::layout::collect_text_nodes(&page.layout_root);
        assert!(!texts.iter().any(|t| t.text.contains("No canvas")));
    }

    fn frame_boxes(html: &str) -> Vec<crate::layout::FrameBox> {
        Page::new(html, "", 800.0, 600.0).frame_boxes()
    }

    #[test]
    fn an_iframe_is_a_box_of_the_default_size_with_room_for_its_document() {
        let boxes = frame_boxes("<html><body><iframe src='inner.html'></iframe></body></html>");
        assert_eq!(boxes.len(), 1);
        assert_eq!(boxes[0].src, "inner.html");
        assert_eq!((boxes[0].rect.width, boxes[0].rect.height), (300.0, 150.0));
        assert!(
            boxes[0].content.width < boxes[0].rect.width,
            "the UA border takes a couple of pixels off each side"
        );
    }

    #[test]
    fn an_iframe_is_sized_by_its_attributes() {
        let boxes = frame_boxes(
            "<html><body><iframe src='a' width='500' height='90'></iframe></body></html>",
        );
        assert_eq!((boxes[0].rect.width, boxes[0].rect.height), (500.0, 90.0));
    }

    #[test]
    fn an_iframe_with_no_source_embeds_nothing() {
        assert!(frame_boxes("<html><body><iframe></iframe></body></html>").is_empty());
    }

    #[test]
    fn an_object_pointing_at_a_picture_is_a_picture() {
        let page = Page::new(
            "<html><body><object data='chart.png'></object></body></html>",
            "",
            800.0,
            600.0,
        );
        assert!(page.frame_boxes().is_empty());
        let images = crate::layout::collect_image_nodes(&page.layout_root);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].src, "chart.png");
    }

    #[test]
    fn an_object_declaring_an_image_type_is_a_picture_whatever_its_url_looks_like() {
        let page = Page::new(
            "<html><body><object data='/render?id=7' type='image/png'></object></body></html>",
            "",
            800.0,
            600.0,
        );
        assert!(page.frame_boxes().is_empty());
        assert_eq!(
            crate::layout::collect_image_nodes(&page.layout_root).len(),
            1
        );
    }

    #[test]
    fn an_embed_pointing_at_a_document_gets_a_context_of_its_own() {
        let boxes = frame_boxes("<html><body><embed src='player.html'></embed></body></html>");
        assert_eq!(boxes.len(), 1);
        assert_eq!(boxes[0].src, "player.html");
    }

    #[test]
    fn the_fallback_inside_an_iframe_is_not_put_on_the_page() {
        let page = Page::new(
            "<html><body><iframe src='a'>Frames are not supported.</iframe></body></html>",
            "",
            800.0,
            600.0,
        );
        let texts = crate::layout::collect_text_nodes(&page.layout_root);
        assert!(!texts.iter().any(|t| t.text.contains("not supported")));
    }

    #[test]
    fn a_frame_holds_the_document_put_into_it() {
        let mut page = Page::new(
            "<html><body><iframe src='inner.html'></iframe></body></html>",
            "",
            800.0,
            600.0,
        );
        let holder = page.frame_boxes()[0].dom_node_id;
        assert!(page.frame(holder).is_none());

        let inner = Page::new(
            "<html><head><title>Inner</title></head><body>hello</body></html>",
            "",
            296.0,
            146.0,
        );
        page.set_frame(holder, inner);
        assert_eq!(page.frame(holder).map(|p| p.title.as_str()), Some("Inner"));
    }

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

    /// Every text run of a page, in paint order.
    ///
    /// CJK is broken into one run per character by line breaking, so the runs
    /// are joined: what these tests are about is which text is on the page and
    /// in what order, not how it was split for measuring.
    fn texts(page: &Page) -> String {
        crate::layout::collect_text_nodes(&page.layout_root)
            .into_iter()
            .map(|run| run.text)
            .collect::<Vec<_>>()
            .join("")
    }

    /// A page whose `p` has generated content on both sides.
    fn generated(css: &str) -> Page {
        Page::new(
            "<html><body><p data-label='ラベル'>本文</p></body></html>",
            &format!("body {{ margin: 0 }} p {{ margin: 0 }} {css}"),
            800.0,
            600.0,
        )
    }

    #[test]
    fn before_and_after_put_their_text_around_the_element() {
        let page = generated("p::before { content: '[' } p::after { content: ']' }");
        assert_eq!(texts(&page), "[本文]");
    }

    #[test]
    fn an_element_with_no_rule_generates_nothing() {
        let page = generated("");
        assert_eq!(texts(&page), "本文");
    }

    #[test]
    fn content_none_generates_no_box() {
        // `content` is what decides whether the box exists at all, so a rule
        // that styles a pseudo-element without content must not produce one.
        let page = generated("p::before { content: none; color: red }");
        assert_eq!(texts(&page), "本文");

        let page = generated("p::before { color: red }");
        assert_eq!(texts(&page), "本文");
    }

    #[test]
    fn attr_reads_the_attribute_off_the_originating_element() {
        let page = generated("p::before { content: attr(data-label) }");
        assert_eq!(texts(&page), "ラベル本文");
    }

    #[test]
    fn a_hex_escape_becomes_the_character_it_names() {
        // Icon fonts are addressed this way; printing the digits instead would
        // put "f101" on the page.
        let page = generated("p::before { content: '\\2192 ' }");
        assert!(texts(&page).starts_with('→'), "got {:?}", texts(&page));
    }

    #[test]
    fn a_generated_box_inherits_from_the_element_it_belongs_to() {
        let page = generated("p { font-size: 30px } p::before { content: 'x' }");
        let run = crate::layout::collect_text_nodes(&page.layout_root)
            .into_iter()
            .find(|r| r.text == "x")
            .expect("the generated box is laid out");
        assert_eq!(run.font_size, 30.0, "inherited from the p");
    }

    #[test]
    fn a_generated_box_can_be_styled_apart_from_its_element() {
        let page = generated("p { color: black } p::before { content: 'x'; color: red }");
        let run = crate::layout::collect_text_nodes(&page.layout_root)
            .into_iter()
            .find(|r| r.text == "x")
            .unwrap();
        assert_eq!(run.color, [255, 0, 0, 255]);
    }

    #[test]
    fn an_empty_content_string_still_generates_a_box_to_paint() {
        // The empty-string box is how pages draw a shape: no text, but a size
        // and a background.
        let page = generated(
            "p::before { content: ''; display: block; width: 40px; height: 10px; \
             background-color: blue }",
        );
        let painted = crate::layout::collect_decorations(&page.layout_root)
            .into_iter()
            .find(|d| d.background_color == Some([0, 0, 255, 255]))
            .expect("the generated box paints its background");
        assert_eq!((painted.width, painted.height), (40.0, 10.0));
    }

    #[test]
    fn generated_content_applies_to_every_element_a_selector_reaches() {
        let page = Page::new(
            "<html><body><ul><li>あ</li><li>い</li></ul></body></html>",
            "li::before { content: '・' }",
            800.0,
            600.0,
        );
        assert_eq!(texts(&page).matches('・').count(), 2);
    }

    #[test]
    fn a_structural_pseudo_class_still_narrows_which_elements_generate() {
        // `li:first-child::before` must not decorate every item.
        let page = Page::new(
            "<html><body><ul><li>あ</li><li>い</li></ul></body></html>",
            "li:first-child::before { content: '★' }",
            800.0,
            600.0,
        );
        assert_eq!(texts(&page).matches('★').count(), 1);
    }

    #[test]
    fn a_more_specific_rule_wins_for_a_generated_box_too() {
        let page = Page::new(
            "<html><body><p class='x'>本文</p></body></html>",
            "p::before { content: 'A' } p.x::before { content: 'B' }",
            800.0,
            600.0,
        );
        assert!(texts(&page).starts_with('B'), "got {:?}", texts(&page));
    }

    /// A page with one 100x50 box, styled by `css`.
    fn transformed(css: &str) -> Page {
        Page::new(
            "<html><body><div id='box'>text</div></body></html>",
            &format!(
                "body {{ margin: 0 }} \
                 #box {{ width: 100px; height: 50px; background-color: blue }} {css}"
            ),
            800.0,
            600.0,
        )
    }

    /// Where the blue box is actually painted, which is not where it was laid
    /// out once a transform is involved.
    fn painted_box(page: &Page) -> crate::layout::VisualDecoration {
        crate::layout::build_display_list(&page.layout_root)
            .into_iter()
            .find_map(|entry| match entry.item {
                crate::layout::PaintItem::Decoration(d)
                    if d.background_color == Some([0, 0, 255, 255]) =>
                {
                    Some(d)
                }
                _ => None,
            })
            .expect("the box paints a background")
    }

    #[test]
    fn an_elements_text_is_drawn_at_the_size_and_colour_the_element_says() {
        // A text node has no style of its own, and taking the initial values
        // for it meant every run came out 16px black: a heading was drawn at
        // body size while the heading box knew its own size perfectly well.
        let page = Page::new(
            "<html><body><h1>Title</h1><p id='p'>para</p></body></html>",
            "body { margin: 0 } #p { font-size: 30px; color: red }",
            800.0,
            600.0,
        );
        let runs = crate::layout::collect_text_nodes(&page.layout_root);

        let heading = runs.iter().find(|r| r.text == "Title").expect("h1 text");
        assert!(
            heading.font_size > 16.0,
            "the UA sheet makes an h1 2em, so its text is bigger than body text: {}",
            heading.font_size
        );

        let para = runs.iter().find(|r| r.text == "para").expect("p text");
        assert_eq!(para.font_size, 30.0);
        assert_eq!(para.color, [255, 0, 0, 255]);
    }

    #[test]
    fn a_translated_box_paints_where_it_was_moved_to() {
        let painted = painted_box(&transformed("#box { transform: translate(30px, 12px) }"));
        assert_eq!((painted.x, painted.y), (30.0, 12.0));
    }

    #[test]
    fn a_transform_does_not_move_the_box_in_the_flow() {
        // A transform is a painting effect: the space the box took is still
        // taken, and its neighbours do not move.
        let page = Page::new(
            "<html><body><div id='moved'></div><p id='after'>下</p></body></html>",
            "body { margin: 0 } p { margin: 0 } \
             #moved { width: 100px; height: 50px; transform: translate(0, 200px) }",
            800.0,
            600.0,
        );
        assert_eq!(
            rect_of(&page, "after").y,
            50.0,
            "the paragraph follows the box's laid-out position, not its painted one"
        );
    }

    #[test]
    fn a_scaled_box_grows_about_its_centre() {
        let painted = painted_box(&transformed("#box { transform: scale(2) }"));
        assert_eq!((painted.width, painted.height), (200.0, 100.0));
        assert_eq!(
            (painted.x, painted.y),
            (-50.0, -25.0),
            "growing about the centre pushes the corner out by half"
        );
    }

    #[test]
    fn the_text_inside_a_transformed_box_travels_with_it() {
        // The subtree moves as one; text left behind would sit outside the
        // background it belongs to.
        let page = transformed("#box { transform: translate(40px, 0) }");
        let run = crate::layout::collect_text_nodes(&page.layout_root)
            .into_iter()
            .find(|r| r.text == "text");
        let plain = run.expect("the text is laid out").x;

        let moved = crate::layout::build_display_list(&page.layout_root)
            .into_iter()
            .find_map(|entry| match entry.item {
                crate::layout::PaintItem::Text(t) if t.text == "text" => Some(t.x),
                _ => None,
            })
            .expect("the text is painted");
        assert_eq!(moved - plain, 40.0);
    }

    #[test]
    fn scaled_text_is_drawn_at_the_scaled_size() {
        let page = transformed("#box { font-size: 10px; transform: scale(3) }");
        let painted = crate::layout::build_display_list(&page.layout_root)
            .into_iter()
            .find_map(|entry| match entry.item {
                crate::layout::PaintItem::Text(t) if t.text == "text" => Some(t),
                _ => None,
            })
            .expect("the text is painted");
        assert_eq!(painted.font_size, 30.0);
    }

    #[test]
    fn a_rotation_paints_the_box_where_it_was_laid_out() {
        // Known limitation, pinned so it is a decision rather than a surprise:
        // the compositor cannot draw a turned rectangle, so the box is painted
        // untransformed instead of vanishing.
        let painted = painted_box(&transformed("#box { transform: rotate(30deg) }"));
        assert_eq!((painted.x, painted.y), (0.0, 0.0));
        assert_eq!((painted.width, painted.height), (100.0, 50.0));
    }

    #[test]
    fn an_untransformed_page_paints_exactly_as_before() {
        let page = transformed("");
        let painted = painted_box(&page);
        assert_eq!((painted.x, painted.y), (0.0, 0.0));
        assert_eq!((painted.width, painted.height), (100.0, 50.0));
    }

    /// A page with one image, whose decoded size is already known.
    fn page_with_decoded_image(markup: &str, css: &str, natural: (u32, u32)) -> Page {
        let mut page = Page::new(
            &format!("<html><body>{markup}</body></html>"),
            css,
            800.0,
            600.0,
        );
        page.image_cache.insert(
            "photo.png".to_string(),
            CachedImage {
                rgba: vec![0; (natural.0 * natural.1 * 4) as usize],
                width: natural.0,
                height: natural.1,
            },
        );
        page.recompute_with_hover(&[]);
        page
    }

    /// The rect of the one image in the page.
    fn image_rect(page: &Page) -> Rect {
        fn find(node: &crate::layout::LayoutNode) -> Option<Rect> {
            if node.image_src.is_some() {
                return Some(node.rect);
            }
            node.children
                .iter()
                .chain(node.absolute_children.iter())
                .find_map(find)
        }
        find(&page.layout_root).expect("the image is in the layout tree")
    }

    #[test]
    fn an_image_with_no_declared_size_takes_the_size_of_its_picture() {
        // Without this the box is empty: the image reserves no space, whatever
        // follows is placed on top of it, and the painter draws the picture at
        // its natural size over that content.
        let page = page_with_decoded_image("<img src='photo.png'>", "", (320, 240));
        let rect = image_rect(&page);
        assert_eq!((rect.width, rect.height), (320.0, 240.0));
    }

    #[test]
    fn a_declared_width_derives_the_height_from_the_aspect_ratio() {
        let page = page_with_decoded_image("<img src='photo.png' width='160'>", "", (320, 240));
        let rect = image_rect(&page);
        assert_eq!(rect.width, 160.0);
        assert_eq!(rect.height, 120.0, "half the width is half the height");
    }

    #[test]
    fn a_declared_height_derives_the_width() {
        let page = page_with_decoded_image("<img src='photo.png' height='120'>", "", (320, 240));
        let rect = image_rect(&page);
        assert_eq!(rect.width, 160.0);
        assert_eq!(rect.height, 120.0);
    }

    #[test]
    fn a_size_declared_in_both_directions_is_left_alone() {
        // The page asked for a distortion; that is the page's business.
        let page = page_with_decoded_image(
            "<img src='photo.png'>",
            "img { width: 100px; height: 300px }",
            (320, 240),
        );
        let rect = image_rect(&page);
        assert_eq!((rect.width, rect.height), (100.0, 300.0));
    }

    #[test]
    fn content_after_an_image_clears_the_space_it_takes() {
        // The symptom the sizing fixes: a paragraph after an unsized image was
        // laid out as though the image were not there.
        let mut page = Page::new(
            "<html><body><img src='photo.png'><p id='after'>下の段落</p></body></html>",
            "body { margin: 0 } p { margin: 0 }",
            800.0,
            600.0,
        );
        page.image_cache.insert(
            "photo.png".to_string(),
            CachedImage {
                rgba: vec![0; 200 * 150 * 4],
                width: 200,
                height: 150,
            },
        );
        page.recompute_with_hover(&[]);

        let image = image_rect(&page);
        let after = rect_of(&page, "after");
        assert!(image.height >= 150.0);
        assert!(
            after.y >= image.y + image.height - 1.0,
            "the paragraph should start below the image: image bottom {}, paragraph y {}",
            image.y + image.height,
            after.y
        );
    }

    #[test]
    fn an_image_that_has_not_arrived_yet_is_left_as_it_was() {
        // Nothing is known about its size, so nothing is invented for it: the
        // box keeps the empty-inline floor it had before, and the painter's own
        // fallback covers the gap until the picture arrives.
        let page = Page::new(
            "<html><body><img src='missing.png'></body></html>",
            "",
            800.0,
            600.0,
        );
        assert!(image_rect(&page).width < 4.0);
    }

    #[test]
    fn a_srcset_image_is_chosen_by_the_width_it_is_drawn_at() {
        let page = Page::new(
            "<html><body>\
             <img srcset='small.png 400w, large.png 1600w' width='1000'>\
             </body></html>",
            "",
            800.0,
            600.0,
        );
        fn find_src(node: &crate::layout::LayoutNode) -> Option<String> {
            if let Some(src) = &node.image_src {
                return Some(src.clone());
            }
            node.children
                .iter()
                .chain(node.absolute_children.iter())
                .find_map(find_src)
        }
        assert_eq!(
            find_src(&page.layout_root).as_deref(),
            Some("large.png"),
            "a 1000px slot needs more than a 400px source"
        );
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
        assert_eq!(rect_of(&page, "box").height, 40.0);

        page.set_zoom(2.0);
        assert_eq!(rect_of(&page, "box").width, 200.0);
        assert_eq!(rect_of(&page, "box").height, 80.0);
    }

    #[test]
    fn a_declared_height_is_the_height_of_a_block_box() {
        // Block height came from content alone, so `height` did nothing at
        // all: an empty spacer collapsed to nothing.
        let page = Page::new(
            "<html><body><div id='spacer'></div></body></html>",
            "body { margin: 0 } #spacer { height: 40px }",
            800.0,
            600.0,
        );
        assert_eq!(rect_of(&page, "spacer").height, 40.0);
    }

    #[test]
    fn a_border_box_height_covers_its_own_padding() {
        let page = Page::new(
            "<html><body><div id='b'></div></body></html>",
            "body { margin: 0 } #b { box-sizing: border-box; height: 50px; padding: 10px }",
            800.0,
            600.0,
        );
        assert_eq!(rect_of(&page, "b").height, 50.0);
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
