//! Rasterising the `filter` property.
//!
//! The compositor paints a filtered element's whole subtree into a buffer of
//! its own; this module is what happens to that buffer before it is put back
//! onto the page. Pixels are premultiplied RGBA, the same as everywhere else in
//! the composite bitmap, which is what lets a blur average the four channels
//! together without haloing on transparent edges.

use crate::css::FilterFn;

/// A rectangle of the composite bitmap, in whole pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Region {
    /// The region grown by `amount` on every side.
    pub fn inflate(self, amount: f32) -> Self {
        let amount = amount.ceil() as i32;
        Self {
            x: self.x - amount,
            y: self.y - amount,
            width: self.width + amount * 2,
            height: self.height + amount * 2,
        }
    }

    /// The part of this region that lies inside a `width` × `height` bitmap.
    pub fn clamp_to(self, width: u32, height: u32) -> Self {
        let x = self.x.clamp(0, width as i32);
        let y = self.y.clamp(0, height as i32);
        let right = (self.x + self.width).clamp(0, width as i32);
        let bottom = (self.y + self.height).clamp(0, height as i32);
        Self {
            x,
            y,
            width: (right - x).max(0),
            height: (bottom - y).max(0),
        }
    }

    pub fn is_empty(self) -> bool {
        self.width <= 0 || self.height <= 0
    }
}

/// A detached copy of one region of a bitmap.
struct Tile {
    pixels: Vec<u8>,
    width: usize,
    height: usize,
}

impl Tile {
    fn read(buffer: &[u8], buf_width: u32, region: Region) -> Self {
        let width = region.width as usize;
        let height = region.height as usize;
        let mut pixels = vec![0u8; width * height * 4];
        for row in 0..height {
            let src = ((region.y as usize + row) * buf_width as usize + region.x as usize) * 4;
            let dst = row * width * 4;
            pixels[dst..dst + width * 4].copy_from_slice(&buffer[src..src + width * 4]);
        }
        Self {
            pixels,
            width,
            height,
        }
    }

    fn write(&self, buffer: &mut [u8], buf_width: u32, region: Region) {
        for row in 0..self.height {
            let dst = ((region.y as usize + row) * buf_width as usize + region.x as usize) * 4;
            let src = row * self.width * 4;
            buffer[dst..dst + self.width * 4]
                .copy_from_slice(&self.pixels[src..src + self.width * 4]);
        }
    }
}

/// Run a `filter` list over one region of the composite bitmap.
///
/// The region is expected to already carry whatever slack
/// [`crate::css::Filter::outset`] asked for; a blur that reaches past it is cut
/// off at the edge rather than wrapping.
pub fn apply(
    buffer: &mut [u8],
    buf_width: u32,
    buf_height: u32,
    region: Region,
    filters: &[FilterFn],
) {
    let region = region.clamp_to(buf_width, buf_height);
    if region.is_empty() || filters.is_empty() {
        return;
    }

    let mut tile = Tile::read(buffer, buf_width, region);
    for function in filters {
        apply_one(&mut tile, *function);
    }
    tile.write(buffer, buf_width, region);
}

fn apply_one(tile: &mut Tile, function: FilterFn) {
    match function {
        FilterFn::Blur(sigma) => blur(&mut tile.pixels, tile.width, tile.height, sigma),
        FilterFn::DropShadow {
            dx,
            dy,
            blur: sigma,
            color,
        } => drop_shadow(tile, dx, dy, sigma, color),
        // `opacity` is the one function that fades the layer rather than only
        // recolouring it, so alpha travels with the colour channels.
        FilterFn::Opacity(amount) => opacity(&mut tile.pixels, amount),
        // Everything else is a per-pixel colour change, and they compose by
        // multiplying their matrices — but they are cheap enough one at a time
        // that keeping them separate is clearer than folding them together.
        other => color_matrix(&mut tile.pixels, matrix_for(other)),
    }
}

/// The 4×5 colour matrix — RGB rows plus a constant column — of a per-pixel
/// filter function.
///
/// Alpha is left alone by all of these except `opacity`, which is handled by
/// scaling the premultiplied pixel whole, so only the RGB rows are modelled.
type ColorMatrix = [[f32; 4]; 3];

fn matrix_for(function: FilterFn) -> ColorMatrix {
    // Rec. 709 luminance, which is what the filter effects spec uses.
    const LR: f32 = 0.2126;
    const LG: f32 = 0.7152;
    const LB: f32 = 0.0722;

    match function {
        FilterFn::Grayscale(amount) => {
            let a = amount.clamp(0.0, 1.0);
            let keep = 1.0 - a;
            [
                [LR * a + keep, LG * a, LB * a, 0.0],
                [LR * a, LG * a + keep, LB * a, 0.0],
                [LR * a, LG * a, LB * a + keep, 0.0],
            ]
        }
        FilterFn::Sepia(amount) => {
            let a = amount.clamp(0.0, 1.0);
            let keep = 1.0 - a;
            [
                [0.393 * a + keep, 0.769 * a, 0.189 * a, 0.0],
                [0.349 * a, 0.686 * a + keep, 0.168 * a, 0.0],
                [0.272 * a, 0.534 * a, 0.131 * a + keep, 0.0],
            ]
        }
        FilterFn::Saturate(amount) => {
            let s = amount.max(0.0);
            [
                [LR + (1.0 - LR) * s, LG - LG * s, LB - LB * s, 0.0],
                [LR - LR * s, LG + (1.0 - LG) * s, LB - LB * s, 0.0],
                [LR - LR * s, LG - LG * s, LB + (1.0 - LB) * s, 0.0],
            ]
        }
        FilterFn::HueRotate(radians) => {
            let (sin, cos) = radians.sin_cos();
            [
                [
                    LR + cos * (1.0 - LR) - sin * LR,
                    LG - cos * LG - sin * LG,
                    LB - cos * LB + sin * (1.0 - LB),
                    0.0,
                ],
                [
                    LR - cos * LR + sin * 0.143,
                    LG + cos * (1.0 - LG) + sin * 0.140,
                    LB - cos * LB - sin * 0.283,
                    0.0,
                ],
                [
                    LR - cos * LR - sin * (1.0 - LR),
                    LG - cos * LG + sin * LG,
                    LB + cos * (1.0 - LB) + sin * LB,
                    0.0,
                ],
            ]
        }
        FilterFn::Invert(amount) => {
            let a = amount.clamp(0.0, 1.0);
            let scale = 1.0 - 2.0 * a;
            [
                [scale, 0.0, 0.0, a],
                [0.0, scale, 0.0, a],
                [0.0, 0.0, scale, a],
            ]
        }
        FilterFn::Brightness(amount) => {
            let b = amount.max(0.0);
            [[b, 0.0, 0.0, 0.0], [0.0, b, 0.0, 0.0], [0.0, 0.0, b, 0.0]]
        }
        FilterFn::Contrast(amount) => {
            let c = amount.max(0.0);
            let shift = 0.5 - c * 0.5;
            [
                [c, 0.0, 0.0, shift],
                [0.0, c, 0.0, shift],
                [0.0, 0.0, c, shift],
            ]
        }
        // Handled elsewhere; identity keeps the compiler happy without
        // changing any pixel.
        FilterFn::Opacity(_) | FilterFn::Blur(_) | FilterFn::DropShadow { .. } => IDENTITY,
    }
}

/// The colour matrix that changes nothing.
const IDENTITY: ColorMatrix = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

/// Fade a whole layer, alpha included.
fn opacity(pixels: &mut [u8], amount: f32) {
    color_matrix_with_alpha(pixels, IDENTITY, amount.clamp(0.0, 1.0));
}

fn color_matrix(pixels: &mut [u8], matrix: ColorMatrix) {
    color_matrix_with_alpha(pixels, matrix, 1.0);
}

/// Apply a colour matrix to unpremultiplied colour, then put the alpha back.
fn color_matrix_with_alpha(pixels: &mut [u8], matrix: ColorMatrix, alpha_scale: f32) {
    for pixel in pixels.chunks_exact_mut(4) {
        let a = pixel[3] as f32 / 255.0;
        if a <= 0.0 {
            pixel[3] = 0;
            continue;
        }
        // Undo the premultiply so the matrix sees the colour the author wrote,
        // not one already faded towards transparent black.
        let r = pixel[0] as f32 / 255.0 / a;
        let g = pixel[1] as f32 / 255.0 / a;
        let b = pixel[2] as f32 / 255.0 / a;

        let out = [
            matrix[0][0] * r + matrix[0][1] * g + matrix[0][2] * b + matrix[0][3],
            matrix[1][0] * r + matrix[1][1] * g + matrix[1][2] * b + matrix[1][3],
            matrix[2][0] * r + matrix[2][1] * g + matrix[2][2] * b + matrix[2][3],
        ];

        let a = (a * alpha_scale).clamp(0.0, 1.0);
        for channel in 0..3 {
            pixel[channel] = (out[channel].clamp(0.0, 1.0) * a * 255.0).round() as u8;
        }
        pixel[3] = (a * 255.0).round() as u8;
    }
}

/// Approximate a Gaussian blur with three box blurs, as SVG filters do.
pub fn blur(pixels: &mut [u8], width: usize, height: usize, sigma: f32) {
    if sigma <= 0.0 || width == 0 || height == 0 {
        return;
    }
    // The box width that best matches a Gaussian of this deviation.
    let d = (sigma * 3.0 * (std::f32::consts::TAU).sqrt() / 4.0 + 0.5).floor();
    let radius = ((d / 2.0).round() as usize).max(1);

    let mut scratch = vec![0u8; pixels.len()];
    for _ in 0..3 {
        box_blur_horizontal(pixels, &mut scratch, width, height, radius);
        box_blur_vertical(&scratch, pixels, width, height, radius);
    }
}

fn box_blur_horizontal(src: &[u8], dst: &mut [u8], width: usize, height: usize, radius: usize) {
    let window = (radius * 2 + 1) as u32;
    for y in 0..height {
        let row = y * width * 4;
        let mut sums = [0u32; 4];
        // Prime the running sum with the window centred on x = 0, clamping the
        // edge pixel outwards rather than letting transparency leak in.
        for offset in 0..=radius {
            let index = row + offset.min(width - 1) * 4;
            for c in 0..4 {
                sums[c] += src[index + c] as u32;
            }
        }
        for c in 0..4 {
            sums[c] += src[row + c] as u32 * radius as u32;
        }

        for x in 0..width {
            let out = row + x * 4;
            for c in 0..4 {
                dst[out + c] = (sums[c] / window) as u8;
            }
            let leaving = row + x.saturating_sub(radius) * 4;
            let entering = row + (x + radius + 1).min(width - 1) * 4;
            for c in 0..4 {
                sums[c] = sums[c] + src[entering + c] as u32 - src[leaving + c] as u32;
            }
        }
    }
}

fn box_blur_vertical(src: &[u8], dst: &mut [u8], width: usize, height: usize, radius: usize) {
    let window = (radius * 2 + 1) as u32;
    let stride = width * 4;
    for x in 0..width {
        let column = x * 4;
        let mut sums = [0u32; 4];
        for offset in 0..=radius {
            let index = column + offset.min(height - 1) * stride;
            for c in 0..4 {
                sums[c] += src[index + c] as u32;
            }
        }
        for c in 0..4 {
            sums[c] += src[column + c] as u32 * radius as u32;
        }

        for y in 0..height {
            let out = column + y * stride;
            for c in 0..4 {
                dst[out + c] = (sums[c] / window) as u8;
            }
            let leaving = column + y.saturating_sub(radius) * stride;
            let entering = column + (y + radius + 1).min(height - 1) * stride;
            for c in 0..4 {
                sums[c] = sums[c] + src[entering + c] as u32 - src[leaving + c] as u32;
            }
        }
    }
}

/// Draw a blurred, offset silhouette of the layer underneath itself.
fn drop_shadow(tile: &mut Tile, dx: f32, dy: f32, sigma: f32, color: [u8; 4]) {
    let (width, height) = (tile.width, tile.height);
    if width == 0 || height == 0 {
        return;
    }

    // The shadow is the layer's alpha channel, in the shadow's colour.
    let mut shadow = vec![0u8; tile.pixels.len()];
    let tint = [
        color[0] as f32 / 255.0,
        color[1] as f32 / 255.0,
        color[2] as f32 / 255.0,
    ];
    let tint_alpha = color[3] as f32 / 255.0;
    for (out, src) in shadow.chunks_exact_mut(4).zip(tile.pixels.chunks_exact(4)) {
        let a = (src[3] as f32 / 255.0) * tint_alpha;
        out[0] = (tint[0] * a * 255.0) as u8;
        out[1] = (tint[1] * a * 255.0) as u8;
        out[2] = (tint[2] * a * 255.0) as u8;
        out[3] = (a * 255.0) as u8;
    }
    blur(&mut shadow, width, height, sigma);

    let offset_x = dx.round() as i32;
    let offset_y = dy.round() as i32;
    let mut out = vec![0u8; tile.pixels.len()];
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let dst = ((y as usize) * width + x as usize) * 4;
            let (sx, sy) = (x - offset_x, y - offset_y);
            if sx >= 0 && sy >= 0 && sx < width as i32 && sy < height as i32 {
                let src = ((sy as usize) * width + sx as usize) * 4;
                out[dst..dst + 4].copy_from_slice(&shadow[src..src + 4]);
            }
        }
    }

    // The element paints over its own shadow, premultiplied source-over.
    for (under, over) in out.chunks_exact_mut(4).zip(tile.pixels.chunks_exact(4)) {
        let inv = 1.0 - over[3] as f32 / 255.0;
        for c in 0..4 {
            under[c] = (over[c] as f32 + under[c] as f32 * inv).min(255.0) as u8;
        }
    }
    tile.pixels = out;
}

/// Composite one region of `src` over the same region of `dst`.
///
/// Both are premultiplied, so this is a plain source-over. Used to put a
/// filtered layer back onto the page.
pub fn composite_region(
    dst: &mut [u8],
    src: &[u8],
    buf_width: u32,
    buf_height: u32,
    region: Region,
) {
    let region = region.clamp_to(buf_width, buf_height);
    if region.is_empty() {
        return;
    }
    for row in 0..region.height as usize {
        let start = ((region.y as usize + row) * buf_width as usize + region.x as usize) * 4;
        for column in 0..region.width as usize {
            let index = start + column * 4;
            let inv = 1.0 - src[index + 3] as f32 / 255.0;
            for c in 0..4 {
                dst[index + c] =
                    (src[index + c] as f32 + dst[index + c] as f32 * inv).min(255.0) as u8;
            }
        }
    }
}

/// Set one region of a bitmap back to transparent black.
pub fn clear_region(buffer: &mut [u8], buf_width: u32, buf_height: u32, region: Region) {
    let region = region.clamp_to(buf_width, buf_height);
    if region.is_empty() {
        return;
    }
    for row in 0..region.height as usize {
        let start = ((region.y as usize + row) * buf_width as usize + region.x as usize) * 4;
        buffer[start..start + region.width as usize * 4].fill(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: usize, height: usize, color: [u8; 4]) -> Vec<u8> {
        color.repeat(width * height)
    }

    #[test]
    fn grayscale_flattens_a_colour_to_its_luminance() {
        let mut pixels = solid(2, 2, [255, 0, 0, 255]);
        color_matrix(&mut pixels, matrix_for(FilterFn::Grayscale(1.0)));
        // 0.2126 of full red, on every channel.
        assert_eq!(pixels[0], 54);
        assert_eq!(pixels[0], pixels[1]);
        assert_eq!(pixels[1], pixels[2]);
        assert_eq!(pixels[3], 255, "alpha is untouched");
    }

    #[test]
    fn grayscale_of_zero_changes_nothing() {
        let original = solid(2, 2, [10, 120, 240, 255]);
        let mut pixels = original.clone();
        color_matrix(&mut pixels, matrix_for(FilterFn::Grayscale(0.0)));
        assert_eq!(pixels, original);
    }

    #[test]
    fn invert_turns_black_into_white() {
        let mut pixels = solid(1, 1, [0, 0, 0, 255]);
        color_matrix(&mut pixels, matrix_for(FilterFn::Invert(1.0)));
        assert_eq!(&pixels[..3], &[255, 255, 255]);
    }

    #[test]
    fn brightness_scales_the_colour_and_clamps_at_white() {
        let mut pixels = solid(1, 1, [100, 100, 100, 255]);
        color_matrix(&mut pixels, matrix_for(FilterFn::Brightness(2.0)));
        assert_eq!(&pixels[..3], &[200, 200, 200]);

        let mut bright = solid(1, 1, [200, 200, 200, 255]);
        color_matrix(&mut bright, matrix_for(FilterFn::Brightness(4.0)));
        assert_eq!(&bright[..3], &[255, 255, 255]);
    }

    #[test]
    fn opacity_fades_the_layer_rather_than_only_its_colour() {
        let mut pixels = solid(1, 1, [255, 255, 255, 255]);
        opacity(&mut pixels, 0.5);
        assert_eq!(pixels[3], 128, "alpha halves");
        assert_eq!(pixels[0], 128, "and so does the premultiplied colour");
    }

    #[test]
    fn a_colour_matrix_leaves_transparent_pixels_transparent() {
        let mut pixels = solid(1, 1, [0, 0, 0, 0]);
        color_matrix(&mut pixels, matrix_for(FilterFn::Invert(1.0)));
        assert_eq!(pixels, vec![0, 0, 0, 0]);
    }

    #[test]
    fn blur_spreads_a_lone_bright_pixel_into_its_neighbours() {
        let width = 9;
        let height = 9;
        let mut pixels = vec![0u8; width * height * 4];
        let centre = (4 * width + 4) * 4;
        pixels[centre..centre + 4].copy_from_slice(&[255, 255, 255, 255]);

        blur(&mut pixels, width, height, 2.0);

        assert!(pixels[centre + 3] < 255, "the centre is spread thinner");
        let neighbour = (4 * width + 5) * 4;
        assert!(pixels[neighbour + 3] > 0, "the neighbour picks some up");
    }

    #[test]
    fn blur_of_a_flat_field_leaves_it_flat() {
        let mut pixels = solid(8, 8, [40, 80, 120, 255]);
        blur(&mut pixels, 8, 8, 3.0);
        for pixel in pixels.chunks_exact(4) {
            assert_eq!(pixel, [40, 80, 120, 255], "edges clamp outwards");
        }
    }

    #[test]
    fn a_region_is_clamped_to_the_bitmap_it_names() {
        let region = Region {
            x: -5,
            y: -5,
            width: 20,
            height: 20,
        }
        .clamp_to(10, 10);
        assert_eq!(
            region,
            Region {
                x: 0,
                y: 0,
                width: 10,
                height: 10
            }
        );
    }

    #[test]
    fn apply_only_touches_the_region_it_is_given() {
        let mut buffer = solid(4, 4, [255, 0, 0, 255]);
        apply(
            &mut buffer,
            4,
            4,
            Region {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
            },
            &[FilterFn::Grayscale(1.0)],
        );
        let pixel = |x: usize, y: usize| buffer[(y * 4 + x) * 4];
        assert_eq!(pixel(0, 0), 54, "inside the region, greyed");
        assert_eq!(pixel(3, 3), 255, "outside it, untouched");
    }

    #[test]
    fn drop_shadow_paints_beside_the_element_without_covering_it() {
        let width = 12;
        let height = 12;
        let mut tile = Tile {
            pixels: vec![0u8; width * height * 4],
            width,
            height,
        };
        for y in 2..6 {
            for x in 2..6 {
                let index = (y * width + x) * 4;
                tile.pixels[index..index + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }

        drop_shadow(&mut tile, 3.0, 3.0, 0.0, [0, 0, 0, 255]);

        let alpha = |x: usize, y: usize| tile.pixels[(y * width + x) * 4 + 3];
        assert_eq!(alpha(3, 3), 255, "the element still covers its own pixels");
        assert_eq!(
            tile.pixels[(3 * width + 3) * 4],
            255,
            "and is still white there"
        );
        assert_eq!(alpha(7, 7), 255, "the shadow lands down and to the right");
        assert_eq!(tile.pixels[(7 * width + 7) * 4], 0, "and is black");
    }

    #[test]
    fn composite_region_lays_one_layer_over_another() {
        let mut dst = solid(2, 2, [0, 0, 255, 255]);
        let src = solid(2, 2, [255, 0, 0, 255]);
        composite_region(
            &mut dst,
            &src,
            2,
            2,
            Region {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
            },
        );
        assert_eq!(&dst[..4], &[255, 0, 0, 255], "an opaque layer wins");
    }

    #[test]
    fn clear_region_empties_only_what_it_names() {
        let mut buffer = solid(4, 1, [9, 9, 9, 255]);
        clear_region(
            &mut buffer,
            4,
            1,
            Region {
                x: 1,
                y: 0,
                width: 2,
                height: 1,
            },
        );
        assert_eq!(&buffer[0..4], &[9, 9, 9, 255]);
        assert_eq!(&buffer[4..12], &[0; 8]);
        assert_eq!(&buffer[12..16], &[9, 9, 9, 255]);
    }

    #[test]
    fn opacity_reaches_the_whole_layer_through_apply() {
        let mut buffer = solid(2, 2, [255, 255, 255, 255]);
        apply(
            &mut buffer,
            2,
            2,
            Region {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
            },
            &[FilterFn::Opacity(0.5)],
        );
        assert_eq!(buffer[3], 128);
    }
}
