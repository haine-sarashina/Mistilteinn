//! Paint the benchmark page into a PNG, with no window and no GPU.
//!
//! ```text
//! cargo run --release --example render_png -- <fixture-dir> out.png [w] [h] [--no-images]
//! ```
//!
//! The same display list the app paints, composited by the same painter, so a
//! difference here is a difference on screen. Pictures are fetched the way the
//! app fetches them, because a render without them cannot answer "is this
//! image missing?" — which is the question a screenshot beside Chrome most
//! often raises. `--no-images` skips the network for a quick layout check.
use futures::StreamExt;
use mistilteinn::page::{CachedImage, Page};

async fn fetch_images(requests: Vec<(String, f32, f32)>) -> Vec<(String, CachedImage)> {
    futures::stream::iter(requests.into_iter().map(|(src, req_w, req_h)| async move {
        let bytes = mistilteinn::network::fetch_image(&src).await.ok()?;
        if let Ok(img) = image::load_from_memory(&bytes) {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            return Some((src, rgba.into_raw(), w, h));
        }
        let svg = std::str::from_utf8(&bytes).ok()?;
        let (rgba, w, h) = mistilteinn::render::render_svg_to_rgba(svg, req_w, req_h)?;
        Some((src, rgba, w, h))
    }))
    .buffer_unordered(8)
    .filter_map(|r| async move {
        r.map(|(src, rgba, width, height)| {
            (
                src,
                CachedImage {
                    rgba,
                    width,
                    height,
                },
            )
        })
    })
    .collect()
    .await
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1).filter(|a| a != "--no-images");
    let skip_images = std::env::args().any(|a| a == "--no-images");
    let dir = args.next().expect("fixture dir");
    let out = args.next().unwrap_or_else(|| "render.png".into());
    let w: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(1565);
    let h: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(1140);

    let html = std::fs::read_to_string(format!("{dir}/wiki.html")).unwrap();
    let css = std::fs::read_to_string(format!("{dir}/wiki.css")).unwrap();
    let mut page = Page::new(&html, &css, w as f32, h as f32);
    // The app knows where the page came from, and protocol-relative URLs like
    // `//upload.wikimedia.org/...` cannot be resolved without it.
    page.page_url =
        "https://ja.wikipedia.org/wiki/%E3%83%A1%E3%82%A4%E3%83%B3%E3%83%9A%E3%83%BC%E3%82%B8"
            .into();

    if !skip_images {
        let viewport = mistilteinn::layout::Rect::new(0.0, 0.0, w as f32, h as f32);
        let wanted = page.pending_image_requests(viewport);
        println!("fetching {} images", wanted.len());
        let fetched = fetch_images(wanted).await;
        println!("decoded {}", fetched.len());
        for (src, img) in fetched {
            page.image_cache.insert(src, img);
        }
        // Sizes are known now, so lay the page out again — as the app does.
        page.recompute_with_hover(&[]);
    }

    let display_list = mistilteinn::layout::build_display_list_with_scroll(
        &page.layout_root,
        (0.0, 0.0),
        (w as f32, h as f32),
    );

    // White page under everything, as a browser paints its canvas.
    let mut buffer = vec![255u8; (w * h * 4) as usize];
    let base = page.base_url();
    let lookup = |src: &str| -> Option<&CachedImage> {
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
