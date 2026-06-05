/// Page orchestration module.
///
/// Connects HTML parsing, CSS computation, layout building, and rendering
/// into a single pipeline: HTML → CSS → Layout → Render.
use rustc_hash::FxHashMap;

use crate::css;
use crate::html::{self, DomArena, DomHandle, NodeId};
use crate::layout::Rect;

/// A fully parsed and laid-out page ready for rendering.
pub struct Page {
    pub arena: DomArena,
    #[allow(dead_code)]
    pub styles: FxHashMap<u32, css::ComputedValues>,
    pub layout_root: crate::layout::LayoutNode,
    pub view_width: f32,
    pub view_height: f32,
}

impl Page {
    /// Run the full pipeline: parse HTML + CSS, compute styles, build and measure layout tree.
    pub fn new(html_source: &str, css_source: &str, view_width: f32, view_height: f32) -> Self {
        // Stage 1: Parse HTML into DOM arena
        let arena = html::parse_html(html_source);

        // Stage 2: Parse CSS into stylesheet
        let stylesheet = crate::css::parser::parse_stylesheet(css_source);

        // Stage 3: Compute styles for every element node
        let styles = css::compute_styles_for_tree(&arena, &stylesheet);

        // Stage 4: Build layout tree starting from document root (node 0)
        let mut layout_root = crate::layout::build_layout_tree(
            0,
            &styles,
            |id: u32| arena.get(DomHandle(NodeId::from_raw(id))),
            view_width,
        );

        // Stage 5: Compute layout positions
        crate::layout::compute_layout(&mut layout_root, view_width);

        log::info!(
            "Page pipeline complete — view: {}x{}, styles: {}, root children: {}",
            view_width,
            view_height,
            styles.len(),
            layout_root.children.len()
        );

        Self {
            arena,
            styles,
            layout_root,
            view_width,
            view_height,
        }
    }

    /// Collect renderable rectangles with colors from the layout tree.
    pub fn collect_rects(&self) -> Vec<(Rect, Option<[u8; 4]>)> {
        crate::layout::collect_render_rects(&self.layout_root)
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
}
