//! The bitmap behind a `<canvas>`, and the drawing operations that fill it.
//!
//! A canvas is the one element whose content the page writes rather than
//! declares, so it needs a surface of its own that survives between scripts and
//! is composited like a picture. Everything here is CPU work on premultiplied
//! RGBA, matching the composite bitmap so the result can be blitted straight
//! into it.

/// A canvas's backing store.
#[derive(Debug, Clone, PartialEq)]
pub struct Surface {
    pub width: u32,
    pub height: u32,
    /// Premultiplied RGBA, row-major.
    pub pixels: Vec<u8>,
}

impl Surface {
    /// A new, fully transparent surface.
    pub fn new(width: u32, height: u32) -> Self {
        let width = width.clamp(1, 8192);
        let height = height.clamp(1, 8192);
        Self {
            width,
            height,
            pixels: vec![0u8; (width * height * 4) as usize],
        }
    }

    /// Resize, discarding what was drawn.
    ///
    /// Setting a canvas's `width` or `height` clears it, which is what the HTML
    /// spec says and what pages rely on to wipe a frame.
    pub fn resize(&mut self, width: u32, height: u32) {
        *self = Surface::new(width, height);
    }

    /// Erase a rectangle back to transparent black.
    pub fn clear_rect(&mut self, x: f32, y: f32, width: f32, height: f32) {
        self.for_each_in(x, y, width, height, |pixel| pixel.copy_from_slice(&[0; 4]));
    }

    /// Paint a rectangle in a solid colour.
    pub fn fill_rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: [u8; 4]) {
        let mut path = Path::default();
        path.rect(x, y, width, height);
        self.fill_path(&path, color);
    }

    /// Paint the outline of a rectangle.
    pub fn stroke_rect(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: [u8; 4],
        line_width: f32,
    ) {
        let mut path = Path::default();
        path.rect(x, y, width, height);
        self.stroke_path(&path, color, line_width);
    }

    /// Draw this surface into the composite bitmap, scaled to a box.
    ///
    /// Both sides are premultiplied, so this is a plain source-over — unlike a
    /// decoded picture, which arrives with straight alpha and has to be
    /// multiplied on the way in.
    pub fn blit_scaled(
        &self,
        dest: &mut [u8],
        dest_width: u32,
        dest_height: u32,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let (origin_x, origin_y) = (x.round() as i32, y.round() as i32);
        for row in 0..height.round() as i32 {
            let target_y = origin_y + row;
            if target_y < 0 || target_y >= dest_height as i32 {
                continue;
            }
            let source_y =
                (((row as f32 / height) * self.height as f32) as u32).min(self.height - 1);
            for column in 0..width.round() as i32 {
                let target_x = origin_x + column;
                if target_x < 0 || target_x >= dest_width as i32 {
                    continue;
                }
                let source_x =
                    (((column as f32 / width) * self.width as f32) as u32).min(self.width - 1);

                let source = ((source_y * self.width + source_x) * 4) as usize;
                let target = ((target_y as usize) * dest_width as usize + target_x as usize) * 4;
                let inverse = 1.0 - self.pixels[source + 3] as f32 / 255.0;
                for channel in 0..4 {
                    dest[target + channel] = (self.pixels[source + channel] as f32
                        + dest[target + channel] as f32 * inverse)
                        .min(255.0) as u8;
                }
            }
        }
    }

    fn for_each_in(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        mut apply: impl FnMut(&mut [u8]),
    ) {
        let x0 = x.floor().max(0.0) as u32;
        let y0 = y.floor().max(0.0) as u32;
        let x1 = ((x + width).ceil().max(0.0) as u32).min(self.width);
        let y1 = ((y + height).ceil().max(0.0) as u32).min(self.height);
        for row in y0..y1 {
            for column in x0..x1 {
                let index = ((row * self.width + column) * 4) as usize;
                apply(&mut self.pixels[index..index + 4]);
            }
        }
    }

    /// Fill a path, using the non-zero winding rule as canvas does by default.
    pub fn fill_path(&mut self, path: &Path, color: [u8; 4]) {
        let edges = path.edges(true);
        self.scan_fill(&edges, color);
    }

    /// Stroke a path by filling a quadrilateral along each of its segments.
    ///
    /// Joins are squared off rather than mitred or rounded: at the line widths
    /// a page actually draws, the difference is a pixel at a corner, and the
    /// simpler geometry keeps every stroke going through the same fill.
    pub fn stroke_path(&mut self, path: &Path, color: [u8; 4], line_width: f32) {
        let half = (line_width.max(0.1)) / 2.0;
        let mut outline = Path::default();

        for subpath in &path.subpaths {
            let points = subpath.drawn_points();
            for pair in points.windows(2) {
                let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
                let (dx, dy) = (x1 - x0, y1 - y0);
                let length = (dx * dx + dy * dy).sqrt();
                if length < 1e-6 {
                    continue;
                }
                // The segment's normal, scaled to half the line width.
                let (nx, ny) = (-dy / length * half, dx / length * half);
                outline.move_to(x0 + nx, y0 + ny);
                outline.line_to(x1 + nx, y1 + ny);
                outline.line_to(x1 - nx, y1 - ny);
                outline.line_to(x0 - nx, y0 - ny);
                outline.close();

                // A square patch at the joint, so consecutive segments do not
                // leave a notch between them.
                outline.rect(x1 - half, y1 - half, half * 2.0, half * 2.0);
            }
        }

        // Overlapping quads must not darken each other where they meet, so the
        // whole outline is filled in one pass under the non-zero rule — and for
        // that to be a union rather than a set of holes, every piece has to
        // wind the same way round. Which way a quad comes out depends on which
        // direction its segment ran in, so they are squared up first.
        outline.make_windings_agree();
        let edges = outline.edges(true);
        self.scan_fill(&edges, color);
    }

    /// Rasterise a set of edges by scanning them, four sub-rows to a pixel.
    ///
    /// Coverage is accumulated per pixel rather than written per sample, which
    /// is what gives a diagonal edge a soft step instead of a staircase.
    fn scan_fill(&mut self, edges: &[Edge], color: [u8; 4]) {
        if edges.is_empty() || color[3] == 0 {
            return;
        }
        const SAMPLES: usize = 4;

        let top = edges
            .iter()
            .map(|e| e.y0.min(e.y1))
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as u32;
        let bottom = (edges
            .iter()
            .map(|e| e.y0.max(e.y1))
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .max(0.0) as u32)
            .min(self.height);

        let mut coverage = vec![0f32; self.width as usize];
        let mut crossings: Vec<(f32, i32)> = Vec::new();

        for row in top..bottom {
            coverage.iter_mut().for_each(|value| *value = 0.0);

            for sample in 0..SAMPLES {
                let y = row as f32 + (sample as f32 + 0.5) / SAMPLES as f32;
                crossings.clear();
                for edge in edges {
                    let (low, high) = (edge.y0.min(edge.y1), edge.y0.max(edge.y1));
                    if y < low || y >= high {
                        continue;
                    }
                    let t = (y - edge.y0) / (edge.y1 - edge.y0);
                    crossings.push((edge.x0 + t * (edge.x1 - edge.x0), edge.winding));
                }
                if crossings.len() < 2 {
                    continue;
                }
                crossings.sort_by(|a, b| a.0.total_cmp(&b.0));

                // Non-zero winding: a span is inside wherever the running sum
                // of edge directions is not zero.
                let mut winding = 0;
                for pair in crossings.windows(2) {
                    winding += pair[0].1;
                    if winding != 0 {
                        add_span(&mut coverage, pair[0].0, pair[1].0, 1.0 / SAMPLES as f32);
                    }
                }
            }

            for (column, amount) in coverage.iter().enumerate() {
                if *amount <= 0.001 {
                    continue;
                }
                let index = ((row * self.width) as usize + column) * 4;
                blend(&mut self.pixels[index..index + 4], color, amount.min(1.0));
            }
        }
    }
}

/// Add horizontal coverage between two x positions, with fractional ends.
fn add_span(coverage: &mut [f32], from: f32, to: f32, weight: f32) {
    let width = coverage.len() as f32;
    let from = from.clamp(0.0, width);
    let to = to.clamp(0.0, width);
    if to <= from {
        return;
    }
    let first = from.floor() as usize;
    let last = (to.ceil() as usize).min(coverage.len());
    for (column, value) in coverage.iter_mut().enumerate().take(last).skip(first) {
        let left = (column as f32).max(from);
        let right = ((column + 1) as f32).min(to);
        if right > left {
            *value += (right - left) * weight;
        }
    }
}

/// Twice the signed area of a closed polygon; the sign is its direction.
fn signed_area(points: &[(f32, f32)]) -> f32 {
    let mut total = 0.0;
    for index in 0..points.len() {
        let (x0, y0) = points[index];
        let (x1, y1) = points[(index + 1) % points.len()];
        total += x0 * y1 - x1 * y0;
    }
    total
}

/// Composite a colour over a premultiplied pixel at the given coverage.
fn blend(pixel: &mut [u8], color: [u8; 4], coverage: f32) {
    let alpha = (color[3] as f32 / 255.0) * coverage;
    if alpha <= 0.0 {
        return;
    }
    let inverse = 1.0 - alpha;
    for channel in 0..3 {
        pixel[channel] =
            (color[channel] as f32 * alpha + pixel[channel] as f32 * inverse).min(255.0) as u8;
    }
    pixel[3] = (255.0 * alpha + pixel[3] as f32 * inverse).min(255.0) as u8;
}

/// One straight edge of a flattened path, with the direction it was walked in.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Edge {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    /// +1 downwards, -1 upwards — what the non-zero rule counts.
    winding: i32,
}

/// One connected run of a path.
#[derive(Debug, Clone, Default, PartialEq)]
struct Subpath {
    points: Vec<(f32, f32)>,
    closed: bool,
}

impl Subpath {
    /// The points a stroke walks, with the closing segment included.
    fn drawn_points(&self) -> Vec<(f32, f32)> {
        let mut points = self.points.clone();
        if self.closed && points.len() > 2 {
            points.push(points[0]);
        }
        points
    }
}

/// A canvas path: straight runs, with curves already flattened into them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Path {
    subpaths: Vec<Subpath>,
}

impl Path {
    pub fn is_empty(&self) -> bool {
        self.subpaths.iter().all(|sub| sub.points.len() < 2)
    }

    pub fn move_to(&mut self, x: f32, y: f32) {
        self.subpaths.push(Subpath {
            points: vec![(x, y)],
            closed: false,
        });
    }

    pub fn line_to(&mut self, x: f32, y: f32) {
        match self.subpaths.last_mut() {
            Some(subpath) => subpath.points.push((x, y)),
            // A `lineTo` with no current point starts one, as canvas does.
            None => self.move_to(x, y),
        }
    }

    pub fn close(&mut self) {
        if let Some(subpath) = self.subpaths.last_mut() {
            subpath.closed = true;
        }
    }

    /// Add a closed rectangle as its own subpath.
    pub fn rect(&mut self, x: f32, y: f32, width: f32, height: f32) {
        self.move_to(x, y);
        self.line_to(x + width, y);
        self.line_to(x + width, y + height);
        self.line_to(x, y + height);
        self.close();
    }

    /// Add an arc, flattened into short straight runs.
    ///
    /// Continues the current subpath when there is one, which is what makes
    /// `moveTo` then `arc` draw a pie slice rather than two separate shapes.
    pub fn arc(
        &mut self,
        cx: f32,
        cy: f32,
        radius: f32,
        start: f32,
        end: f32,
        counterclockwise: bool,
    ) {
        if radius <= 0.0 {
            return;
        }
        let mut sweep = end - start;
        if counterclockwise {
            while sweep > 0.0 {
                sweep -= std::f32::consts::TAU;
            }
        } else {
            while sweep < 0.0 {
                sweep += std::f32::consts::TAU;
            }
        }
        sweep = sweep.clamp(-std::f32::consts::TAU, std::f32::consts::TAU);

        // One segment per few degrees, more for a larger circle, so the flats
        // stay under a pixel at the sizes a page draws.
        let steps = ((radius * sweep.abs()).ceil() as usize).clamp(8, 512);
        for step in 0..=steps {
            let angle = start + sweep * (step as f32 / steps as f32);
            let (x, y) = (cx + radius * angle.cos(), cy + radius * angle.sin());
            if step == 0 && self.subpaths.is_empty() {
                self.move_to(x, y);
            } else {
                self.line_to(x, y);
            }
        }
    }

    /// Turn every subpath the same way round.
    ///
    /// Under the non-zero rule two overlapping shapes wound in opposite
    /// directions cancel where they meet; wound the same way they combine. A
    /// stroke wants the second, so its pieces are made to agree.
    fn make_windings_agree(&mut self) {
        for subpath in &mut self.subpaths {
            if signed_area(&subpath.points) < 0.0 {
                subpath.points.reverse();
            }
        }
    }

    /// Flatten to edges. `implicitly_close` joins each subpath's last point to
    /// its first, which is what filling does whether or not `closePath` was
    /// called.
    fn edges(&self, implicitly_close: bool) -> Vec<Edge> {
        let mut edges = Vec::new();
        for subpath in &self.subpaths {
            if subpath.points.len() < 2 {
                continue;
            }
            let mut points = subpath.points.clone();
            if implicitly_close && points.first() != points.last() {
                points.push(points[0]);
            }
            for pair in points.windows(2) {
                let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
                if (y0 - y1).abs() < 1e-6 {
                    // A horizontal edge is never crossed by a scanline.
                    continue;
                }
                edges.push(Edge {
                    x0,
                    y0,
                    x1,
                    y1,
                    winding: if y1 > y0 { 1 } else { -1 },
                });
            }
        }
        edges
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: [u8; 4] = [255, 0, 0, 255];

    fn alpha_at(surface: &Surface, x: u32, y: u32) -> u8 {
        surface.pixels[((y * surface.width + x) * 4 + 3) as usize]
    }

    fn red_at(surface: &Surface, x: u32, y: u32) -> u8 {
        surface.pixels[((y * surface.width + x) * 4) as usize]
    }

    #[test]
    fn a_new_surface_is_transparent() {
        let surface = Surface::new(4, 4);
        assert!(surface.pixels.iter().all(|byte| *byte == 0));
    }

    #[test]
    fn fill_rect_paints_inside_and_not_outside() {
        let mut surface = Surface::new(10, 10);
        surface.fill_rect(2.0, 2.0, 4.0, 4.0, RED);
        assert_eq!(alpha_at(&surface, 3, 3), 255);
        assert_eq!(red_at(&surface, 3, 3), 255);
        assert_eq!(alpha_at(&surface, 0, 0), 0);
        assert_eq!(alpha_at(&surface, 8, 8), 0);
    }

    #[test]
    fn clear_rect_erases_what_was_there() {
        let mut surface = Surface::new(8, 8);
        surface.fill_rect(0.0, 0.0, 8.0, 8.0, RED);
        surface.clear_rect(2.0, 2.0, 3.0, 3.0);
        assert_eq!(alpha_at(&surface, 3, 3), 0);
        assert_eq!(alpha_at(&surface, 6, 6), 255, "outside the cleared box");
    }

    #[test]
    fn resizing_wipes_the_canvas() {
        let mut surface = Surface::new(4, 4);
        surface.fill_rect(0.0, 0.0, 4.0, 4.0, RED);
        surface.resize(6, 3);
        assert_eq!((surface.width, surface.height), (6, 3));
        assert!(surface.pixels.iter().all(|byte| *byte == 0));
    }

    #[test]
    fn a_filled_triangle_covers_its_inside_only() {
        let mut surface = Surface::new(20, 20);
        let mut path = Path::default();
        path.move_to(2.0, 2.0);
        path.line_to(18.0, 2.0);
        path.line_to(2.0, 18.0);
        path.close();
        surface.fill_path(&path, RED);

        assert_eq!(alpha_at(&surface, 4, 4), 255, "well inside");
        assert_eq!(alpha_at(&surface, 16, 16), 0, "past the hypotenuse");
    }

    #[test]
    fn an_unclosed_path_is_still_filled() {
        let mut surface = Surface::new(20, 20);
        let mut path = Path::default();
        path.move_to(2.0, 2.0);
        path.line_to(18.0, 2.0);
        path.line_to(18.0, 18.0);
        surface.fill_path(&path, RED);
        assert_eq!(alpha_at(&surface, 15, 10), 255);
    }

    #[test]
    fn a_diagonal_edge_is_softened_rather_than_stepped() {
        let mut surface = Surface::new(32, 32);
        let mut path = Path::default();
        path.move_to(0.0, 0.0);
        path.line_to(32.0, 0.0);
        path.line_to(0.0, 32.0);
        path.close();
        surface.fill_path(&path, RED);

        let has_partial = (0..32)
            .flat_map(|y| (0..32).map(move |x| (x, y)))
            .any(|(x, y)| {
                let a = alpha_at(&surface, x, y);
                a > 0 && a < 255
            });
        assert!(has_partial, "the hypotenuse should be antialiased");
    }

    #[test]
    fn a_stroked_line_covers_the_pixels_it_runs_through() {
        let mut surface = Surface::new(20, 20);
        let mut path = Path::default();
        path.move_to(2.0, 10.0);
        path.line_to(18.0, 10.0);
        surface.stroke_path(&path, RED, 4.0);

        assert!(alpha_at(&surface, 10, 10) > 200, "on the line");
        assert_eq!(alpha_at(&surface, 10, 2), 0, "well above it");
    }

    #[test]
    fn a_stroke_does_not_darken_itself_where_two_segments_meet() {
        let mut surface = Surface::new(24, 24);
        let mut path = Path::default();
        path.move_to(4.0, 4.0);
        path.line_to(20.0, 4.0);
        path.line_to(20.0, 20.0);
        surface.stroke_path(&path, [255, 0, 0, 128], 6.0);

        // Half-transparent red laid over itself twice would come out darker at
        // the corner than along either arm.
        let along_arm = alpha_at(&surface, 10, 4);
        let at_corner = alpha_at(&surface, 20, 4);
        assert_eq!(along_arm, at_corner);
    }

    #[test]
    fn stroke_rect_draws_a_frame_and_leaves_the_middle_alone() {
        let mut surface = Surface::new(20, 20);
        surface.stroke_rect(4.0, 4.0, 12.0, 12.0, RED, 2.0);
        assert!(alpha_at(&surface, 10, 4) > 200, "the top edge");
        assert_eq!(alpha_at(&surface, 10, 10), 0, "the middle stays empty");
    }

    #[test]
    fn an_arc_of_a_full_turn_fills_a_disc() {
        let mut surface = Surface::new(40, 40);
        let mut path = Path::default();
        path.arc(20.0, 20.0, 12.0, 0.0, std::f32::consts::TAU, false);
        surface.fill_path(&path, RED);

        assert_eq!(alpha_at(&surface, 20, 20), 255, "the centre");
        assert!(alpha_at(&surface, 20, 10) > 128, "inside the radius");
        assert_eq!(alpha_at(&surface, 20, 2), 0, "outside it");
    }

    #[test]
    fn an_arc_continues_the_subpath_it_is_added_to() {
        // `moveTo` the centre, then an arc: canvas draws a pie slice.
        let mut path = Path::default();
        path.move_to(20.0, 20.0);
        path.arc(20.0, 20.0, 12.0, 0.0, std::f32::consts::FRAC_PI_2, false);
        path.close();

        let mut surface = Surface::new(40, 40);
        surface.fill_path(&path, RED);
        assert_eq!(alpha_at(&surface, 24, 24), 255, "inside the slice");
        assert_eq!(alpha_at(&surface, 12, 12), 0, "the opposite quadrant");
    }

    #[test]
    fn drawing_outside_the_surface_does_not_panic() {
        let mut surface = Surface::new(8, 8);
        surface.fill_rect(-50.0, -50.0, 20.0, 20.0, RED);
        surface.fill_rect(100.0, 100.0, 20.0, 20.0, RED);
        surface.clear_rect(-5.0, -5.0, 100.0, 100.0);
    }

    #[test]
    fn blitting_puts_the_surface_on_the_page_where_it_belongs() {
        let mut surface = Surface::new(4, 4);
        surface.fill_rect(0.0, 0.0, 4.0, 4.0, RED);

        let mut page = vec![0u8; 8 * 8 * 4];
        surface.blit_scaled(&mut page, 8, 8, 2.0, 2.0, 4.0, 4.0);

        let alpha = |x: usize, y: usize| page[(y * 8 + x) * 4 + 3];
        assert_eq!(alpha(3, 3), 255, "inside the box it was drawn into");
        assert_eq!(alpha(0, 0), 0, "and nowhere else");
        assert_eq!(alpha(7, 7), 0);
    }

    #[test]
    fn blitting_a_transparent_surface_leaves_the_page_as_it_was() {
        let surface = Surface::new(4, 4);
        let mut page = vec![9u8; 8 * 8 * 4];
        surface.blit_scaled(&mut page, 8, 8, 0.0, 0.0, 8.0, 8.0);
        assert!(page.iter().all(|byte| *byte == 9));
    }

    #[test]
    fn blitting_off_the_edge_of_the_page_does_not_panic() {
        let mut surface = Surface::new(4, 4);
        surface.fill_rect(0.0, 0.0, 4.0, 4.0, RED);
        let mut page = vec![0u8; 8 * 8 * 4];
        surface.blit_scaled(&mut page, 8, 8, -6.0, -6.0, 4.0, 4.0);
        surface.blit_scaled(&mut page, 8, 8, 20.0, 20.0, 4.0, 4.0);
    }

    #[test]
    fn an_empty_path_paints_nothing() {
        let mut surface = Surface::new(8, 8);
        let path = Path::default();
        assert!(path.is_empty());
        surface.fill_path(&path, RED);
        assert!(surface.pixels.iter().all(|byte| *byte == 0));
    }
}
