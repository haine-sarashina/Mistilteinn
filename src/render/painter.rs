//! Painting a laid-out page onto a bitmap.
//!
//! Separate from the window because a page is painted in more than one place:
//! onto the composite bitmap behind the browser chrome, and onto a sheet of
//! paper when the reader prints. Everything the two have in common lives here,
//! and the only thing that distinguishes them is where layout coordinate
//! (0, 0) lands.

use crate::layout::DisplayItem;
use crate::page::CachedImage;
use crate::render::text::TextRenderer;

/// Paint a display list onto an RGBA bitmap.
///
/// `origin` is where layout coordinate (0, 0) goes: for the window that is the
/// corner of the content pane less however far the page has scrolled, and for
/// a printed sheet it is the top of the strip of page being put on it.
///
/// `lookup_image` resolves a paint item's source to a decoded picture; the
/// caller owns the cache, because a framed document's pictures live in its
/// parent's.
pub fn paint_page<'cache>(
    display_list: &[DisplayItem],
    lookup_image: &dyn Fn(&str) -> Option<&'cache CachedImage>,
    buffer: &mut [u8],
    width: u32,
    height: u32,
    origin: (f32, f32),
) {
    let mut text_renderer = TextRenderer::new();

    // Where a layout rect lands on this bitmap.
    let to_screen_rect = |r: crate::layout::Rect| {
        crate::layout::Rect::new(r.x + origin.0, r.y + origin.1, r.width, r.height)
    };

    // Paint one display item onto whichever buffer is being built: the page
    // itself, or the scratch layer a filtered subtree is drawn into first.
    let mut paint_entry = |target: &mut [u8], entry: &crate::layout::DisplayItem| {
        // An `overflow` clip on an ancestor keeps this item inside that
        // box. The bounds handed to the scissor may be generous — being too
        // large only means more area is correctly restored.
        let clip = entry.clip.map(to_screen_rect);
        let bounds = to_screen_rect(crate::layout::paint_bounds(&entry.item));

        crate::render::with_scissor(
            target,
            width,
            height,
            bounds,
            clip,
            |target_buffer: &mut [u8]| {
                match &entry.item {
                    crate::layout::PaintItem::Decoration(deco) => {
                        let dx = deco.x + origin.0;
                        let dy = deco.y + origin.1;

                        // A masked box paints its colour only where the mask
                        // is opaque, so the flat fill below is skipped: filling
                        // first would put a solid rectangle where the icon is
                        // meant to be, and the mask cannot take it back.
                        let masked = deco.mask_image.is_some();

                        if let Some(bg) = deco.background_color.filter(|_| !masked) {
                            if deco.border_radius > 0.0 {
                                crate::render::draw_rounded_rect_fill(
                                    target_buffer,
                                    width,
                                    height,
                                    dx,
                                    dy,
                                    deco.width,
                                    deco.height,
                                    deco.border_radius,
                                    bg,
                                );
                            } else {
                                crate::render::draw_solid_rect(
                                    target_buffer,
                                    width,
                                    height,
                                    dx,
                                    dy,
                                    deco.width,
                                    deco.height,
                                    bg,
                                );
                            }
                        }

                        // The mask is the shape of the box's colour. Until the
                        // picture arrives there is nothing to shape it with, so
                        // nothing is painted rather than a placeholder block.
                        if let Some(ref src) = deco.mask_image
                            && let Some(cached) = lookup_image(src)
                            && let Some(color) = deco.background_color
                        {
                            crate::render::draw_masked_color(
                                &cached.rgba,
                                cached.width,
                                cached.height,
                                target_buffer,
                                width,
                                height,
                                dx,
                                dy,
                                deco.width,
                                deco.height,
                                deco.mask_size,
                                deco.mask_position,
                                deco.mask_repeat,
                                color,
                            );
                        }

                        // The image paints over the background colour and under the border.
                        if let Some(ref src) = deco.background_image
                            && let Some(cached) = lookup_image(src)
                        {
                            crate::render::draw_background_image(
                                &cached.rgba,
                                cached.width,
                                cached.height,
                                target_buffer,
                                width,
                                height,
                                dx,
                                dy,
                                deco.width,
                                deco.height,
                                deco.background_size,
                                deco.background_position,
                                deco.background_repeat,
                            );
                        }

                        crate::render::draw_rect_borders(
                            target_buffer,
                            width,
                            height,
                            dx,
                            dy,
                            deco.width,
                            deco.height,
                            deco.border_width,
                            deco.border_color,
                            deco.border_style,
                        );
                    }

                    crate::layout::PaintItem::Text(text_info) => {
                        let color_f32: [f32; 4] = [
                            text_info.color[0] as f32 / 255.0,
                            text_info.color[1] as f32 / 255.0,
                            text_info.color[2] as f32 / 255.0,
                            text_info.color[3] as f32 / 255.0,
                        ];

                        // Apply scroll offset and shift by chrome dimensions
                        let text_x = text_info.x + origin.0;
                        let text_y = text_info.y + origin.1;

                        text_renderer.rasterize_to_bitmap_styled(
                            &text_info.text,
                            text_info.font_size,
                            &text_info.font_family,
                            color_f32,
                            text_x,
                            text_y,
                            text_info.width,
                            text_info.text_style,
                            target_buffer,
                            width,
                            height,
                        );
                    }

                    crate::layout::PaintItem::Image(img_info) => {
                        // Skip unpositioned or collapsed small icons at (0, 0).
                        let collapsed_icon = img_info.x <= 0.0
                            && img_info.y <= 0.0
                            && img_info.width < 32.0
                            && img_info.height < 32.0;
                        let Some(cached) = lookup_image(&img_info.src).filter(|_| !collapsed_icon)
                        else {
                            return;
                        };
                        let img_x = img_info.x + origin.0;
                        let img_y = img_info.y + origin.1;
                        let target_w = if img_info.width >= 4.0 {
                            img_info.width
                        } else {
                            cached.width as f32
                        };
                        let target_h = if img_info.height >= 4.0 {
                            img_info.height
                        } else {
                            cached.height as f32
                        };
                        if target_w >= 4.0 && target_h >= 4.0 {
                            crate::render::composite_image_scaled(
                                &cached.rgba,
                                cached.width,
                                cached.height,
                                target_buffer,
                                width,
                                height,
                                img_x,
                                img_y,
                                target_w,
                                target_h,
                            );
                        }
                    }
                }
            },
        );
    };

    // Walk the display list, painting straight onto the page except where a
    // `filter` claims a run of items. Those go onto a scratch layer of their
    // own, are put through the filter, and are composited back — which is
    // what makes the filter apply to the subtree as one picture rather than
    // to each box separately.
    let mut filter_scratch: Vec<u8> = Vec::new();
    let mut index = 0;
    while index < display_list.len() {
        let Some(filter) = display_list[index].filter.clone() else {
            paint_entry(buffer, &display_list[index]);
            index += 1;
            continue;
        };

        let end = display_list[index..]
            .iter()
            .position(|entry| {
                !entry
                    .filter
                    .as_ref()
                    .is_some_and(|other| std::rc::Rc::ptr_eq(other, &filter))
            })
            .map(|offset| index + offset)
            .unwrap_or(display_list.len());
        let group = &display_list[index..end];
        index = end;

        // The area to filter is everything the group paints, grown by how
        // far a blur or a shadow can carry paint outside it.
        let mut region: Option<crate::layout::Rect> = None;
        for entry in group {
            let bounds = to_screen_rect(crate::layout::paint_bounds(&entry.item));
            region = Some(match region {
                None => bounds,
                Some(current) => crate::layout::union_rect(current, bounds),
            });
        }
        let Some(region) = region else { continue };
        let region = crate::render::filter::Region {
            x: region.x.floor() as i32,
            y: region.y.floor() as i32,
            width: region.width.ceil() as i32,
            height: region.height.ceil() as i32,
        }
        .inflate(filter.outset())
        .clamp_to(width, height);
        if region.is_empty() {
            continue;
        }

        if filter_scratch.is_empty() {
            filter_scratch = vec![0u8; (width * height * 4) as usize];
        } else {
            crate::render::filter::clear_region(&mut filter_scratch, width, height, region);
        }
        for entry in group {
            paint_entry(&mut filter_scratch, entry);
        }
        crate::render::filter::apply(
            &mut filter_scratch,
            width,
            height,
            region,
            filter.functions(),
        );
        crate::render::filter::composite_region(buffer, &filter_scratch, width, height, region);
    }
}
