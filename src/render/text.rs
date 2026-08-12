/// Text rendering using Parley for text layout and swash for glyph rasterization.
///
/// This module handles text measurement, glyph layout, and bitmap rasterization.
/// Full GPU rendering (glyph atlas, texture upload) is a follow-up task.
///
/// Current scope:
/// - Font database initialization (system fonts via Fontique)
/// - Text measurement (advance width, line height)
/// - Glyph layout via Parley
/// - Glyph data collection for rendering
/// - Bitmap rasterization via swash (accessed through parley's re-export)
///
/// TODO: Glyph atlas texture, wgpu text render pipeline, draw calls.
use parley::{
    Alignment, AlignmentOptions, FontContext, Layout, LayoutContext, PositionedLayoutItem,
};

/// Glyph information extracted from laid-out text for rendering.
#[derive(Clone, Debug)]
pub struct GlyphInfo {
    /// X position in layout space (pixels from left).
    pub x: f32,
    /// Y position in layout space (pixels from top).
    pub y: f32,
    /// Glyph advance width (for positioning next glyph).
    pub advance: f32,
    /// Font size in pixels.
    pub font_size: f32,
    /// RGBA color (0.0..=1.0).
    pub color: [f32; 4],
}

/// A rasterized glyph bitmap ready for upload to GPU texture.
#[derive(Clone)]
pub struct GlyphBitmap {
    /// X position on the atlas/output surface (pixels from left).
    pub x: u32,
    /// Y position on the atlas/output surface (pixels from top).
    pub y: u32,
    /// Bitmap width in pixels.
    pub width: u32,
    /// Bitmap height in pixels.
    pub height: u32,
    /// Pre-multiplied RGBA pixel data.
    pub rgba: Vec<u8>,
}

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

/// Text renderer using Parley for glyph layout and swash for rasterization.
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
    pub fn measure(&mut self, text: &str, font_size: f32, family: &str) -> (f32, f32) {
        let display_scale = 1.0;
        let mut builder = self
            .layout_ctx
            .ranged_builder(&mut self.font_ctx, text, display_scale);

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
    pub fn layout_runs(&mut self, runs: &[TextRun], _max_width: f32) -> Vec<TextRun> {
        let mut result = Vec::with_capacity(runs.len());

        for run in runs {
            let (advance, line_height) = self.measure(&run.text, run.font_size, &run.family);

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
        let mut builder = self
            .layout_ctx
            .ranged_builder(&mut self.font_ctx, text, display_scale);

        builder.push_default(parley::StyleProperty::FontSize(font_size));
        builder.push_default(parley::StyleProperty::FontStack(family.into()));
        builder.push_default(parley::StyleProperty::LineHeight(1.2));

        let mut layout: Layout<()> = builder.build(text);
        layout.break_all_lines(Some(max_width));
        layout.align(
            Some(max_width),
            Alignment::Start,
            AlignmentOptions::default(),
        );

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

    /// Collect glyph data from laid-out text for rendering.
    ///
    /// Uses Parley to shape and layout the text, then iterates over each
    /// glyph run to extract position, advance, font size, and color info.
    pub fn collect_glyph_data(
        &mut self,
        text: &str,
        font_size: f32,
        family: &str,
        x: f32,
        y: f32,
        max_width: f32,
        color: [f32; 4],
    ) -> Vec<GlyphInfo> {
        let display_scale = 1.0;
        let mut builder = self
            .layout_ctx
            .ranged_builder(&mut self.font_ctx, text, display_scale);

        builder.push_default(parley::StyleProperty::FontSize(font_size));
        builder.push_default(parley::StyleProperty::FontStack(family.into()));
        builder.push_default(parley::StyleProperty::LineHeight(1.2));

        let mut layout: Layout<()> = builder.build(text);
        layout.break_all_lines(Some(max_width));
        layout.align(
            Some(max_width),
            Alignment::Start,
            AlignmentOptions::default(),
        );

        let mut glyphs = Vec::new();
        let mut cursor_x = x;
        let mut line_y = y;

        for line in layout.lines() {
            let metrics = line.metrics();
            let baseline = line_y + metrics.ascent;

            for item in line.items() {
                match item {
                    PositionedLayoutItem::GlyphRun(glyph_run) => {
                        let glyph_x = cursor_x + glyph_run.offset();

                        for g in glyph_run.glyphs() {
                            glyphs.push(GlyphInfo {
                                x: glyph_x + g.x,
                                y: baseline - metrics.ascent,
                                advance: g.advance,
                                font_size,
                                color,
                            });

                            let px_advance = g.advance * display_scale;
                            cursor_x += px_advance;
                        }
                    }
                    PositionedLayoutItem::InlineBox(_) => {
                        // Skip inline boxes for now
                    }
                }
            }

            line_y += metrics.descent + metrics.leading;
            cursor_x = x;
        }

        glyphs
    }

    /// Rasterize laid-out text into an RGBA bitmap buffer using swash.
    ///
    /// Uses Parley to shape and layout the text, then uses swash's Scaler
    /// (via parley's re-export of swash 0.2) to rasterize each glyph into
    /// the destination buffer at the correct position.
    ///
    /// The `buffer` must be large enough to hold `width * height * 4` bytes
    /// (RGBA). Glyphs are drawn with alpha blending against existing content.
    pub fn rasterize_to_bitmap(
        &mut self,
        text: &str,
        font_size: f32,
        family: &str,
        color: [f32; 4],
        origin_x: f32,
        origin_y: f32,
        max_width: f32,
        buffer: &mut [u8],
        width: u32,
        height: u32,
    ) {
        if width == 0 || height == 0 || text.is_empty() {
            return;
        }

        let display_scale = 1.0;
        let mut builder = self
            .layout_ctx
            .ranged_builder(&mut self.font_ctx, text, display_scale);

        builder.push_default(parley::StyleProperty::FontSize(font_size));
        builder.push_default(parley::StyleProperty::FontStack(family.into()));
        builder.push_default(parley::StyleProperty::LineHeight(1.2));

        let mut layout: Layout<()> = builder.build(text);
        layout.break_all_lines(Some(max_width));
        layout.align(
            Some(max_width),
            Alignment::Start,
            AlignmentOptions::default(),
        );

        // swash ScaleContext manages LRU caches for glyph rasterization
        let mut scale_ctx = swash::scale::ScaleContext::new();
        let mut line_y = origin_y;

        for line in layout.lines() {
            let metrics = line.metrics();
            let baseline = line_y + metrics.ascent;
            let mut cursor_x = origin_x;

            for item in line.items() {
                match item {
                    PositionedLayoutItem::GlyphRun(glyph_run) => {
                        // Get the Run from GlyphRun, then access the Font
                        let run = glyph_run.run();
                        let font = run.font();
                        let font_data = font.data.as_ref();

                        // Create a swash FontRef from parley's font data
                        // peniko::Font stores raw bytes in .data and face index in .index
                        let swash_font =
                            match swash::FontRef::from_index(font_data, font.index as usize) {
                                Some(f) => f,
                                None => {
                                    // Advance cursor past this run if we can't load the font
                                    cursor_x += glyph_run.advance() * display_scale;
                                    continue;
                                }
                            };

                        // Build a scaler for this font at the desired size
                        let ppem = (font_size * display_scale) as u32;
                        let mut scaler = scale_ctx.builder(swash_font).size(ppem as f32).build();

                        // Configure rasterization to produce an alpha mask
                        let sources = [swash::scale::Source::Outline];
                        let render = swash::scale::Render::new(&sources);

                        for g in glyph_run.glyphs() {
                            let px = ((cursor_x + g.x).round()) as i32;
                            let py = (baseline - g.y) as i32;

                            // GlyphId from parley uses swash's GlyphId internally
                            if let Some(image) = render.render(&mut scaler, g.id) {
                                Self::draw_glyph_alpha(
                                    &image, px, py, color, buffer, width, height,
                                );
                            }

                            cursor_x += g.advance * display_scale;
                        }
                    }
                    PositionedLayoutItem::InlineBox(_) => {
                        // Skip inline boxes
                    }
                }
            }

            line_y += metrics.descent + metrics.leading;
        }
    }

    /// Draw a swash glyph alpha image into an RGBA buffer with color tinting.
    fn draw_glyph_alpha(
        image: &swash::scale::image::Image,
        glyph_x: i32,
        glyph_y: i32,
        color: [f32; 4],
        buffer: &mut [u8],
        buf_width: u32,
        buf_height: u32,
    ) {
        if image.data.is_empty() || buffer.is_empty() {
            return;
        }

        // Image placement contains the glyph bitmap dimensions and offset
        let width = image.placement.width;
        let height = image.placement.height;
        let stride = width.next_multiple_of(8);

        if (stride * height) as usize > image.data.len() {
            return;
        }

        // Clamp glyph bounds to buffer bounds
        let start_x = glyph_x.max(0) as u32;
        let start_y = glyph_y.max(0) as u32;
        let end_x = (glyph_x + width as i32).min(buf_width as i32) as u32;
        let end_y = (glyph_y + height as i32).min(buf_height as i32) as u32;

        for gy in 0..height.min(end_y.saturating_sub(start_y)) {
            let src_row_start = (gy * stride) as usize;
            let buf_row_y = start_y + gy;

            for gx in 0..width.min(end_x.saturating_sub(start_x)) {
                let buf_x = start_x + gx;
                let buf_idx = (buf_row_y * buf_width * 4 + buf_x * 4) as usize;

                if buf_idx + 3 >= buffer.len() {
                    continue;
                }

                let alpha = image.data[src_row_start + gx as usize] as f32 / 255.0;
                let a = color[3] * alpha;

                // Alpha blend: src over dest
                buffer[buf_idx] = ((color[0] * a * 255.0)
                    + (buffer[buf_idx] as f32 / 255.0 * (1.0 - a)) * 255.0)
                    as u8;
                buffer[buf_idx + 1] = ((color[1] * a * 255.0)
                    + (buffer[buf_idx + 1] as f32 / 255.0 * (1.0 - a)) * 255.0)
                    as u8;
                buffer[buf_idx + 2] = ((color[2] * a * 255.0)
                    + (buffer[buf_idx + 2] as f32 / 255.0 * (1.0 - a)) * 255.0)
                    as u8;
                buffer[buf_idx + 3] = (a * 255.0 
                    + (buffer[buf_idx + 3] as f32 / 255.0 * (1.0 - a)) * 255.0) 
                    as u8;
            }
        }
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

    #[test]
    fn collect_glyph_data_basic() {
        let mut renderer = TextRenderer::new();
        let glyphs = renderer.collect_glyph_data(
            "Hi",
            16.0,
            "sans-serif",
            10.0,
            20.0,
            800.0,
            [0.0, 0.0, 0.0, 1.0],
        );
        assert!(!glyphs.is_empty(), "Should collect glyph data for text");
        // Each glyph should have valid position and advance
        for g in &glyphs {
            assert!(g.x >= 0.0);
            assert!(g.y >= 0.0);
            assert!(g.advance > 0.0, "Glyph advance should be positive");
        }
    }

    #[test]
    fn glyph_info_fields() {
        let glyph = GlyphInfo {
            x: 10.0,
            y: 20.0,
            advance: 8.5,
            font_size: 16.0,
            color: [1.0, 0.0, 0.0, 1.0],
        };
        assert_eq!(glyph.x, 10.0);
        assert_eq!(glyph.advance, 8.5);
        assert_eq!(glyph.color[0], 1.0);
    }

    #[test]
    fn rasterize_to_bitmap_empty() {
        let mut renderer = TextRenderer::new();
        let mut buffer = vec![255u8; 100 * 100 * 4];
        // Empty text should not crash
        renderer.rasterize_to_bitmap(
            "",
            16.0,
            "sans-serif",
            [0.0, 0.0, 0.0, 1.0],
            0.0,
            0.0,
            800.0,
            &mut buffer,
            100,
            100,
        );
    }

    #[test]
    fn rasterize_to_bitmap_zero_size() {
        let mut renderer = TextRenderer::new();
        // Zero-sized buffer should not crash
        renderer.rasterize_to_bitmap(
            "Hello",
            16.0,
            "sans-serif",
            [0.0, 0.0, 0.0, 1.0],
            0.0,
            0.0,
            800.0,
            &mut [],
            0,
            0,
        );
    }

    #[test]
    fn rasterize_to_bitmap_with_text() {
        let mut renderer = TextRenderer::new();
        let size = 200 * 200 * 4;
        let mut buffer = vec![255u8; size];
        // Render black text into white background
        renderer.rasterize_to_bitmap(
            "AB",
            16.0,
            "sans-serif",
            [0.0, 0.0, 0.0, 1.0],
            50.0,
            50.0,
            400.0,
            &mut buffer,
            200,
            200,
        );
        // Should not panic; hard to check pixel values without font guarantees
    }
}
