//! Browser-chrome icons, drawn as shapes rather than typed as characters.
//!
//! The chrome used to spell its icons with characters — `↻` for reload, `☆`
//! for the bookmark star. Whether one appears at all then depends on the
//! system font covering that code point, and on this machine it does not: the
//! reload button and the bookmark star were painted as nothing, so a button
//! that existed and worked looked like empty space. The shaper's fallback does
//! not rescue them either, since these characters are script `Common` and have
//! no fallback family to land in.
//!
//! Drawing them here removes the font from the question entirely. Each icon is
//! a predicate over a point; the sampler below turns that into pixels with a
//! little anti-aliasing, so shapes come out smooth at chrome sizes.

use super::draw_solid_rect;

/// How many samples per pixel axis the icon sampler takes.
///
/// 2×2 is enough to take the staircase off a diagonal at 16px; more is not
/// visible and every icon is repainted on each frame.
const SAMPLES: i32 = 2;

/// Fill the pixels of `rect` for which `inside` holds.
///
/// Coverage is estimated by sampling, so an edge lands as a partly transparent
/// pixel rather than a hard step.
fn fill_shape(
    dest: &mut [u8],
    dest_width: u32,
    dest_height: u32,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: [u8; 4],
    inside: impl Fn(f32, f32) -> bool,
) {
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let step = 1.0 / SAMPLES as f32;
    let total = (SAMPLES * SAMPLES) as f32;

    for py in 0..height.ceil() as i32 {
        for px in 0..width.ceil() as i32 {
            let mut hits = 0;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let sample_x = px as f32 + (sx as f32 + 0.5) * step;
                    let sample_y = py as f32 + (sy as f32 + 0.5) * step;
                    if inside(sample_x, sample_y) {
                        hits += 1;
                    }
                }
            }
            if hits == 0 {
                continue;
            }
            let coverage = hits as f32 / total;
            let shade = [
                color[0],
                color[1],
                color[2],
                (color[3] as f32 * coverage) as u8,
            ];
            draw_solid_rect(
                dest,
                dest_width,
                dest_height,
                x + px as f32,
                y + py as f32,
                1.0,
                1.0,
                shade,
            );
        }
    }
}

/// The reload icon: a circular arrow, open at the top right, with a head.
///
/// `size` is the box it is drawn in; the icon fills it with a small margin.
pub fn reload(
    dest: &mut [u8],
    dest_width: u32,
    dest_height: u32,
    x: f32,
    y: f32,
    size: f32,
    color: [u8; 4],
) {
    let radius = size * 0.36;
    let thickness = (size * 0.1).max(1.2);
    let centre = size / 2.0;

    // The ring, with a gap where the arrow head goes. Angles run clockwise from
    // straight up, which is where the head sits.
    fill_shape(
        dest,
        dest_width,
        dest_height,
        x,
        y,
        size,
        size,
        color,
        |px, py| {
            let dx = px - centre;
            let dy = py - centre;
            let distance = (dx * dx + dy * dy).sqrt();
            if (distance - radius).abs() > thickness / 2.0 {
                return false;
            }
            // Leave the top-right quadrant open so the circle reads as an
            // arrow rather than as a ring.
            !(dx >= 0.0 && dy <= 0.0)
        },
    );

    // The head: a triangle at the top, pointing right, closing the gap.
    let head = size * 0.3;
    let tip_x = centre + radius + head * 0.35;
    let base_x = centre + radius - head * 0.35;
    let top_y = centre - head * 0.55;
    fill_shape(
        dest,
        dest_width,
        dest_height,
        x,
        y,
        size,
        size,
        color,
        |px, py| {
            // Point-in-triangle by the sign of each edge's cross product.
            let (ax, ay) = (tip_x, top_y + head * 0.55);
            let (bx, by) = (base_x, top_y);
            let (cx, cy) = (tip_x + head * 0.35, top_y);
            let sign = |(x1, y1): (f32, f32), (x2, y2): (f32, f32)| {
                (px - x2) * (y1 - y2) - (x1 - x2) * (py - y2)
            };
            let d1 = sign((ax, ay), (bx, by));
            let d2 = sign((bx, by), (cx, cy));
            let d3 = sign((cx, cy), (ax, ay));
            let has_negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
            let has_positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
            !(has_negative && has_positive)
        },
    );
}

/// A five-pointed star: hollow when the page is not saved, solid when it is.
pub fn star(
    dest: &mut [u8],
    dest_width: u32,
    dest_height: u32,
    x: f32,
    y: f32,
    size: f32,
    color: [u8; 4],
    filled: bool,
) {
    let centre = size / 2.0;
    let outer = size * 0.45;
    let inner = outer * 0.42;

    // Ten alternating points, starting at the top.
    let points: Vec<(f32, f32)> = (0..10)
        .map(|i| {
            let radius = if i % 2 == 0 { outer } else { inner };
            let angle = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
            (centre + radius * angle.cos(), centre + radius * angle.sin())
        })
        .collect();

    let in_star = move |px: f32, py: f32| {
        // Even-odd crossing test: a ray to the right crosses the outline an odd
        // number of times exactly when the point is inside.
        let mut inside = false;
        for i in 0..points.len() {
            let (x1, y1) = points[i];
            let (x2, y2) = points[(i + 1) % points.len()];
            if (y1 > py) != (y2 > py) {
                let crossing = x1 + (py - y1) / (y2 - y1) * (x2 - x1);
                if px < crossing {
                    inside = !inside;
                }
            }
        }
        inside
    };

    if filled {
        fill_shape(
            dest,
            dest_width,
            dest_height,
            x,
            y,
            size,
            size,
            color,
            in_star,
        );
        return;
    }

    // Hollow: the outline is the star minus a copy of itself scaled down about
    // its centre, which keeps the stroke even around every point.
    let inset = 0.78;
    fill_shape(
        dest,
        dest_width,
        dest_height,
        x,
        y,
        size,
        size,
        color,
        move |px, py| {
            if !in_star(px, py) {
                return false;
            }
            let inner_x = centre + (px - centre) / inset;
            let inner_y = centre + (py - centre) / inset;
            !in_star(inner_x, inner_y)
        },
    );
}

/// Which way a chevron points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Down,
}

/// A solid triangle, used for navigation buttons and tree expanders.
pub fn chevron(
    dest: &mut [u8],
    dest_width: u32,
    dest_height: u32,
    x: f32,
    y: f32,
    size: f32,
    color: [u8; 4],
    direction: Direction,
) {
    let half = size * 0.3;
    let centre = size / 2.0;

    fill_shape(
        dest,
        dest_width,
        dest_height,
        x,
        y,
        size,
        size,
        color,
        |px, py| {
            // Each triangle is a wedge: the distance across the short axis
            // narrows to nothing at the tip.
            let (along, across) = match direction {
                Direction::Right => (px - (centre - half), py - centre),
                Direction::Left => ((centre + half) - px, py - centre),
                Direction::Down => (py - (centre - half), px - centre),
            };
            along >= 0.0 && along <= half * 2.0 && across.abs() <= half - along / 2.0
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How many pixels of `buffer` have any opacity at all.
    fn painted(buffer: &[u8]) -> usize {
        buffer.chunks(4).filter(|p| p[3] > 0).count()
    }

    fn canvas() -> (Vec<u8>, u32, u32) {
        (vec![0u8; 32 * 32 * 4], 32, 32)
    }

    #[test]
    fn every_icon_paints_something() {
        // The whole point of drawing these rather than typing them: a missing
        // glyph paints nothing, and a button that paints nothing is invisible.
        let (mut buffer, w, h) = canvas();
        reload(&mut buffer, w, h, 4.0, 4.0, 24.0, [255, 255, 255, 255]);
        assert!(painted(&buffer) > 20, "the reload icon should be visible");

        let (mut buffer, w, h) = canvas();
        star(
            &mut buffer,
            w,
            h,
            4.0,
            4.0,
            24.0,
            [255, 255, 255, 255],
            true,
        );
        assert!(painted(&buffer) > 20, "the star should be visible");

        let (mut buffer, w, h) = canvas();
        chevron(
            &mut buffer,
            w,
            h,
            4.0,
            4.0,
            24.0,
            [255, 255, 255, 255],
            Direction::Right,
        );
        assert!(painted(&buffer) > 10, "the chevron should be visible");
    }

    #[test]
    fn a_hollow_star_uses_less_ink_than_a_solid_one() {
        let (mut solid, w, h) = canvas();
        star(&mut solid, w, h, 4.0, 4.0, 24.0, [255, 255, 255, 255], true);
        let (mut hollow, w, h) = canvas();
        star(
            &mut hollow,
            w,
            h,
            4.0,
            4.0,
            24.0,
            [255, 255, 255, 255],
            false,
        );

        assert!(painted(&hollow) > 10, "the outline is still drawn");
        assert!(
            painted(&hollow) < painted(&solid),
            "hollow {} should be lighter than solid {}",
            painted(&hollow),
            painted(&solid)
        );
    }

    #[test]
    fn a_chevron_points_where_it_is_told() {
        // The tip is on the side it points at, so that half carries less ink.
        let ink_in_left_half = |direction| {
            let (mut buffer, w, h) = canvas();
            chevron(
                &mut buffer,
                w,
                h,
                0.0,
                0.0,
                32.0,
                [255, 255, 255, 255],
                direction,
            );
            let mut left = 0;
            let mut right = 0;
            for y in 0..32usize {
                for x in 0..32usize {
                    if buffer[(y * 32 + x) * 4 + 3] > 0 {
                        if x < 16 {
                            left += 1;
                        } else {
                            right += 1;
                        }
                    }
                }
            }
            (left, right)
        };

        let (left, right) = ink_in_left_half(Direction::Right);
        assert!(left > right, "a right-pointing chevron is wide on the left");

        let (left, right) = ink_in_left_half(Direction::Left);
        assert!(right > left, "a left-pointing chevron is wide on the right");
    }

    #[test]
    fn an_icon_drawn_off_the_edge_does_not_panic_or_wrap() {
        // Chrome icons are positioned from window geometry, which can put one
        // partly outside a small window.
        let (mut buffer, w, h) = canvas();
        reload(&mut buffer, w, h, -8.0, -8.0, 24.0, [255, 255, 255, 255]);
        star(
            &mut buffer,
            w,
            h,
            28.0,
            28.0,
            24.0,
            [255, 255, 255, 255],
            true,
        );
        // Nothing to assert beyond surviving; a wrapped write would land in the
        // opposite corner, which `draw_solid_rect` clamps away.
    }

    #[test]
    fn a_zero_sized_icon_draws_nothing() {
        let (mut buffer, w, h) = canvas();
        reload(&mut buffer, w, h, 4.0, 4.0, 0.0, [255, 255, 255, 255]);
        assert_eq!(painted(&buffer), 0);
    }
}

/// The tick inside a checked checkbox.
///
/// Two strokes meeting at the low point, which is what a tick is: a short one
/// down to the left and a long one up to the right.
pub fn checkmark(
    dest: &mut [u8],
    dest_width: u32,
    dest_height: u32,
    x: f32,
    y: f32,
    size: f32,
    color: [u8; 4],
) {
    // A stroke half as thick as a tenth of the box keeps the tick crisp at the
    // 13px a checkbox usually is, and still solid if a page enlarges one.
    let thickness = (size * 0.16).max(1.2);

    fill_shape(
        dest,
        dest_width,
        dest_height,
        x,
        y,
        size,
        size,
        color,
        |px, py| {
            let (px, py) = (px / size, py / size);
            // The corner the two strokes meet at, and their far ends.
            let elbow = (0.42, 0.72);
            let start = (0.22, 0.52);
            let end = (0.78, 0.30);
            let width = thickness / size;
            near_segment(px, py, start, elbow, width) || near_segment(px, py, elbow, end, width)
        },
    );
}

/// Whether a point is within `width` of the line between two others.
fn near_segment(px: f32, py: f32, a: (f32, f32), b: (f32, f32), width: f32) -> bool {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length_squared = dx * dx + dy * dy;
    if length_squared <= f32::EPSILON {
        return false;
    }
    // How far along the segment the nearest point is, clamped to its ends so a
    // stroke stops rather than running on forever.
    let t = (((px - a.0) * dx + (py - a.1) * dy) / length_squared).clamp(0.0, 1.0);
    let (nx, ny) = (a.0 + dx * t, a.1 + dy * t);
    let (ox, oy) = (px - nx, py - ny);
    ox * ox + oy * oy <= (width / 2.0) * (width / 2.0)
}

/// The dot inside a chosen radio button.
pub fn radio_dot(
    dest: &mut [u8],
    dest_width: u32,
    dest_height: u32,
    x: f32,
    y: f32,
    size: f32,
    color: [u8; 4],
) {
    let centre = size / 2.0;
    let radius = size * 0.26;
    fill_shape(
        dest,
        dest_width,
        dest_height,
        x,
        y,
        size,
        size,
        color,
        |px, py| {
            let (dx, dy) = (px - centre, py - centre);
            dx * dx + dy * dy <= radius * radius
        },
    );
}
