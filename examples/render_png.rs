//! Paint the benchmark page into a PNG, with no window and no GPU.
//!
//! `cargo run --release --example render_png -- <fixture-dir> <out.png> [width] [height]`
//!
//! The same display list the app paints, composited by the same painter, so a
//! difference here is a difference on screen. Being able to look at the page
//! without taking a screenshot by hand is what makes "compare against Chrome"
//! a thing that can be repeated.
use mistilteinn::page::Page;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("fixture dir");
    let out = args.next().unwrap_or_else(|| "render.png".into());
    let w: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(1565);
    let h: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(1140);

    let html = std::fs::read_to_string(format!("{dir}/wiki.html")).unwrap();
    let css = std::fs::read_to_string(format!("{dir}/wiki.css")).unwrap();
    let page = Page::new(&html, &css, w as f32, h as f32);

    let display_list = mistilteinn::layout::build_display_list_with_scroll(
        &page.layout_root,
        (0.0, 0.0),
        (w as f32, h as f32),
    );

    // White page under everything, as a browser paints its canvas.
    let mut buffer = vec![255u8; (w * h * 4) as usize];
    let lookup = |src: &str| -> Option<&mistilteinn::page::CachedImage> {
        let base = page.base_url();
        let resolved = if base.is_empty() {
            src.to_string()
        } else {
            mistilteinn::network::resolve_url(&base, src)
        };
        page.image_cache
            .get(&resolved)
            .or_else(|| page.image_cache.get(src))
    };
    mistilteinn::render::painter::paint_page(&display_list, &lookup, &mut buffer, w, h, (0.0, 0.0));

    image::save_buffer(&out, &buffer, w, h, image::ColorType::Rgba8).unwrap();
    println!("wrote {out} ({w}x{h}), {} paint items", display_list.len());
}
