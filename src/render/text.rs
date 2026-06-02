/// Text rendering using Parley for text layout.
///
/// This module handles text measurement and glyph layout.
/// Full GPU rendering (glyph atlas, texture upload) is a follow-up task.
///
/// Current scope:
/// - Font database initialization (system fonts via Fontique)
/// - Text measurement (advance width, line height)
/// - Glyph layout via Parley
///
/// TODO: Glyph atlas texture, wgpu text render pipeline, draw calls.

use parley::{Alignment, AlignmentOptions, FontContext, Layout, LayoutContext, PositionedLayoutItem};

/// A single line of laid-out text and its position.
#[derive(Clone)]
pub struct TextRun {
    /// Text content.
    pub text: String,
    /// Position in layout space (top-left corner).
    pub x: f32,
    pub y: f32,
    /// Font size in pixels.
    pub font_size: f32,
    /// RGBA color (0.0..=1.0).
    pub color: [f32; 4],
    /// Family name.
    pub family: String,
    /// Computed advance width (for layout consumption).
    pub advance: f32,
    /// Line height (ascender + descender).
    pub line_height: f32,
}

/// Text renderer using Parley for glyph layout.
pub struct TextRenderer {
    /// Font context for loading font faces (system fonts auto-discovered).
    font_ctx: FontContext,
    /// Layout context for caching/scratch space.
    layout_ctx: LayoutContext<()>,
}

impl TextRenderer {
    /// Create a new text renderer with system fonts loaded.
    ///
    /// Parley's `FontContext::new()` auto-discovers system fonts via Fontique.
    pub fn new() -> Self {
        Self {
            font_ctx: FontContext::new(),
            layout_ctx: LayoutContext::new(),
        }
    }

    /// Measure a string and return the total advance width and height.
    ///
    /// This is used by the layout engine to compute text box dimensions
    /// before rendering.
    pub fn measure(
        &mut self,
        text: &str,
        font_size: f32,
        family: &str,
    ) -> (f32, f32) {
        let display_scale = 1.0;
        let mut builder = self.layout_ctx.ranged_builder(
            &mut self.font_ctx,
            text,
            display_scale,
        );

        // Set default styles
        builder.push_default(parley::StyleProperty::FontSize(font_size));
        builder.push_default(parley::StyleProperty::FontStack(family.into()));
        builder.push_default(parley::StyleProperty::LineHeight(1.2));

        let mut layout: Layout<()> = builder.build(text);

        // Break lines at max width (no wrapping for measurement)
        layout.break_all_lines(None);
        layout.align(None, Alignment::Start, AlignmentOptions::default());

        let width = layout.max_content_width();
        let height = layout.height();

        (width, height)
    }

    /// Layout text runs and return the computed dimensions.
    ///
    /// Each run is laid out independently. The advance and line_height
    /// fields are populated with computed values.
    pub fn layout_runs(
        &mut self,
        runs: &[TextRun],
        _max_width: f32,
    ) -> Vec<TextRun> {
        let mut result = Vec::with_capacity(runs.len());

        for run in runs {
            let (advance, line_height) = self.measure(
                &run.text,
                run.font_size,
                &run.family,
            );

            result.push(TextRun {
                advance,
                line_height,
                ..run.clone()
            });
        }

        result
    }

    /// Iterate over all glyph runs in a laid-out text.
    ///
    /// Returns (x, y, advance) for each glyph run line.
    /// Used for rendering preparation.
    pub fn layout_text_lines(
        &mut self,
        text: &str,
        font_size: f32,
        family: &str,
        max_width: f32,
    ) -> Vec<(f32, f32, f32)> {
        let display_scale = 1.0;
        let mut builder = self.layout_ctx.ranged_builder(
            &mut self.font_ctx,
            text,
            display_scale,
        );

        builder.push_default(parley::StyleProperty::FontSize(font_size));
        builder.push_default(parley::StyleProperty::FontStack(family.into()));
        builder.push_default(parley::StyleProperty::LineHeight(1.2));

        let mut layout: Layout<()> = builder.build(text);
        layout.break_all_lines(Some(max_width));
        layout.align(Some(max_width), Alignment::Start, AlignmentOptions::default());

        let mut lines = Vec::new();
        for line in layout.lines() {
            let metrics = line.metrics();
            for item in line.items() {
                match item {
                    PositionedLayoutItem::GlyphRun(glyph_run) => {
                        let advance = glyph_run.advance();
                        lines.push((glyph_run.offset(), metrics.offset, advance));
                    }
                    PositionedLayoutItem::InlineBox(_box) => {
                        // Skip inline boxes for now
                    }
                }
            }
        }

        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_renderer_new() {
        let _renderer = TextRenderer::new();
        // Just verify it creates without panic
        assert!(true);
    }

    #[test]
    fn measure_ascii_text() {
        let mut renderer = TextRenderer::new();
        let (advance, height) = renderer.measure("Hello", 16.0, "sans-serif");
        assert!(advance > 0.0, "ASCII text should have positive advance");
        assert!(height > 0.0, "Text should have positive height");
    }

    #[test]
    fn measure_empty_text() {
        let mut renderer = TextRenderer::new();
        let (advance, height) = renderer.measure("", 16.0, "sans-serif");
        assert_eq!(advance, 0.0, "Empty text has zero advance");
        // Empty text may have zero or minimal height
        assert!(height >= 0.0);
    }

    #[test]
    fn layout_single_run() {
        let mut renderer = TextRenderer::new();
        let runs = vec![TextRun {
            text: "Test".to_string(),
            x: 0.0,
            y: 0.0,
            font_size: 14.0,
            color: [0.0, 0.0, 0.0, 1.0],
            family: "sans-serif".to_string(),
            advance: 0.0,
            line_height: 0.0,
        }];
        let laid_out = renderer.layout_runs(&runs, 800.0);
        assert_eq!(laid_out.len(), 1);
        assert!(laid_out[0].advance > 0.0);
    }

    #[test]
    fn layout_text_lines_basic() {
        let mut renderer = TextRenderer::new();
        let lines = renderer.layout_text_lines("Hello World", 16.0, "sans-serif", 800.0);
        assert!(!lines.is_empty(), "Should have at least one line");
    }
}
