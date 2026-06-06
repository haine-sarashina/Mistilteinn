use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::WindowAttributes,
};

use crate::render::text::TextRenderer;
use crate::render::{ColorF, Renderer, layout_to_clip};

/// Main application struct implementing winit's ApplicationHandler trait.
pub struct MistilteinnApp {
    renderer: Option<Renderer>,
    page: Option<crate::page::Page>,
}

impl MistilteinnApp {
    /// Rebuild the render artifacts from the current page and upload to GPU.
    ///
    /// This covers the entire pipeline from layout tree to GPU buffers:
    /// collect rects, convert to clip space, set GPU buffers, collect/rasterize
    /// text nodes, composite image nodes, and upload the composite bitmap.
    fn recompose(&mut self) {
        let Some(ref mut renderer) = self.renderer else {
            return;
        };
        let Some(ref page) = self.page else { return };

        let view_w = page.view_width;
        let view_h = page.view_height;

        // Collect render rectangles from layout tree and convert to clip space
        let rects = page.collect_rects();
        let clip_rects: Vec<_> = rects
            .iter()
            .take(4)
            .filter_map(|(r, c)| {
                if r.width > 0.0 && r.height > 0.0 {
                    Some((
                        layout_to_clip(r.x, r.y, r.width, r.height, view_w, view_h),
                        c,
                    ))
                } else {
                    None
                }
            })
            .collect();

        let render_rects: Vec<_> = clip_rects.iter().map(|(r, _)| *r).collect();
        let render_colors: Vec<_> = clip_rects
            .iter()
            .map(|(_, c)| {
                if let Some(col) = c {
                    crate::render::color_u8_to_f32(*col)
                } else {
                    ColorF {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.0,
                    }
                }
            })
            .collect();

        renderer.set_rects(&render_rects, &render_colors);

        // Collect text nodes from layout tree and rasterize into a bitmap
        let text_nodes = crate::layout::collect_text_nodes(&page.layout_root);

        // Collect image nodes from layout tree (sync recompose does NOT fetch images)
        let image_nodes = crate::layout::collect_image_nodes(&page.layout_root);

        if !text_nodes.is_empty() || !image_nodes.is_empty() {
            let view_width = page.view_width as u32;
            let view_height = page.view_height as u32;

            // Allocate RGBA buffer (transparent background)
            let mut composite_buffer = vec![0u8; (view_width * view_height * 4) as usize];

            // Rasterize text nodes into the composite buffer
            if !text_nodes.is_empty() {
                let mut text_renderer = TextRenderer::new();

                for text_info in &text_nodes {
                    let color_f32: [f32; 4] = [
                        text_info.color[0] as f32 / 255.0,
                        text_info.color[1] as f32 / 255.0,
                        text_info.color[2] as f32 / 255.0,
                        text_info.color[3] as f32 / 255.0,
                    ];

                    text_renderer.rasterize_to_bitmap(
                        &text_info.text,
                        text_info.font_size,
                        "sans-serif",
                        color_f32,
                        text_info.x,
                        text_info.y,
                        text_info.width,
                        &mut composite_buffer,
                        view_width,
                        view_height,
                    );
                }

                log::info!(
                    "Rasterized {} text nodes at {}x{}",
                    text_nodes.len(),
                    view_width,
                    view_height
                );
            }

            // Upload the composite bitmap to GPU
            renderer.set_text_bitmap(view_width, view_height, &composite_buffer);
            log::info!(
                "Composite overlay uploaded: {} text + {} images at {}x{}",
                text_nodes.len(),
                image_nodes.len(),
                view_width,
                view_height
            );
        }
    }

    /// Load a page from HTML and CSS source strings.
    ///
    /// Rebuilds the entire page pipeline (parse, style, layout) then calls
    /// `recompose()` to upload render artifacts to the GPU.
    pub fn load_page(&mut self, html_source: &str, css_source: &str) {
        let w = self.window_width() as f32;
        let h = self.window_height() as f32;

        self.page = Some(crate::page::Page::new(html_source, css_source, w, h));
        self.recompose();

        if let Some(ref mut renderer) = self.renderer {
            if let Err(e) = renderer.render() {
                log::error!("Render after load_page failed: {:?}", e);
            } else {
                log::info!("Page loaded and rendered");
            }
        }
    }

    /// Load a page asynchronously (fetches and composites images).
    ///
    /// Rebuilds the page, composes rectangles and text, then fetches and
    /// composites any image nodes before uploading to GPU.
    pub async fn load_page_async(&mut self, html_source: &str, css_source: &str) {
        let w = self.window_width() as f32;
        let h = self.window_height() as f32;

        let new_page = crate::page::Page::new(html_source, css_source, w, h);
        self.page = Some(new_page);

        // Borrow the page we just stored for composition
        let Some(ref page) = self.page else { return };
        let Some(ref mut renderer) = self.renderer else {
            return;
        };
        let view_w = page.view_width;
        let view_h = page.view_height;

        // Collect render rectangles from layout tree and convert to clip space
        let rects = page.collect_rects();
        let clip_rects: Vec<_> = rects
            .iter()
            .take(4)
            .filter_map(|(r, c)| {
                if r.width > 0.0 && r.height > 0.0 {
                    Some((
                        layout_to_clip(r.x, r.y, r.width, r.height, view_w, view_h),
                        c,
                    ))
                } else {
                    None
                }
            })
            .collect();

        let render_rects: Vec<_> = clip_rects.iter().map(|(r, _)| *r).collect();
        let render_colors: Vec<_> = clip_rects
            .iter()
            .map(|(_, c)| {
                if let Some(col) = c {
                    crate::render::color_u8_to_f32(*col)
                } else {
                    ColorF {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.0,
                    }
                }
            })
            .collect();

        renderer.set_rects(&render_rects, &render_colors);

        // Collect text and image nodes from layout tree
        let text_nodes = crate::layout::collect_text_nodes(&page.layout_root);
        let image_nodes = crate::layout::collect_image_nodes(&page.layout_root);

        if !text_nodes.is_empty() || !image_nodes.is_empty() {
            let view_width = page.view_width as u32;
            let view_height = page.view_height as u32;

            // Allocate RGBA buffer (transparent background)
            let mut composite_buffer = vec![0u8; (view_width * view_height * 4) as usize];

            // Rasterize text nodes into the composite buffer
            if !text_nodes.is_empty() {
                let mut text_renderer = TextRenderer::new();

                for text_info in &text_nodes {
                    let color_f32: [f32; 4] = [
                        text_info.color[0] as f32 / 255.0,
                        text_info.color[1] as f32 / 255.0,
                        text_info.color[2] as f32 / 255.0,
                        text_info.color[3] as f32 / 255.0,
                    ];

                    text_renderer.rasterize_to_bitmap(
                        &text_info.text,
                        text_info.font_size,
                        "sans-serif",
                        color_f32,
                        text_info.x,
                        text_info.y,
                        text_info.width,
                        &mut composite_buffer,
                        view_width,
                        view_height,
                    );
                }

                log::info!(
                    "Rasterized {} text nodes at {}x{}",
                    text_nodes.len(),
                    view_width,
                    view_height
                );
            }

            // Fetch and composite image nodes into the buffer
            for img_info in &image_nodes {
                match reqwest::get(&img_info.src).await {
                    Ok(response) => {
                        let bytes = match response.bytes().await {
                            Ok(b) => b,
                            Err(e) => {
                                log::warn!(
                                    "Failed to read image bytes for {}: {:?}",
                                    img_info.src,
                                    e
                                );
                                continue;
                            }
                        };

                        if let Ok(img) = image::load_from_memory(&bytes) {
                            let rgba = img.to_rgba8();
                            let (iw, ih) = rgba.dimensions();
                            crate::render::composite_image(
                                rgba.as_raw(),
                                iw,
                                ih,
                                &mut composite_buffer,
                                view_width,
                                view_height,
                                img_info.x,
                                img_info.y,
                            );
                            log::info!(
                                "Loaded image: {} ({}x{}) at ({}, {})",
                                img_info.src,
                                iw,
                                ih,
                                img_info.x,
                                img_info.y
                            );
                        } else {
                            log::warn!("Failed to decode image: {}", img_info.src);
                        }
                    }
                    Err(e) => {
                        log::warn!("Failed to fetch image {}: {:?}", img_info.src, e);
                    }
                }
            }

            // Upload the composite bitmap to GPU
            renderer.set_text_bitmap(view_width, view_height, &composite_buffer);
            log::info!(
                "Composite overlay uploaded: {} text + {} images at {}x{}",
                text_nodes.len(),
                image_nodes.len(),
                view_width,
                view_height
            );
        }

        if let Err(e) = renderer.render() {
            log::error!("Render after load_page_async failed: {:?}", e);
        } else {
            log::info!("Page loaded (async) and rendered");
        }
    }

    /// Get the current window width, or default 1280.
    fn window_width(&self) -> u32 {
        self.renderer
            .as_ref()
            .map(|r| r.window().inner_size().width)
            .unwrap_or(1280)
    }

    /// Get the current window height, or default 800.
    fn window_height(&self) -> u32 {
        self.renderer
            .as_ref()
            .map(|r| r.window().inner_size().height)
            .unwrap_or(800)
    }
}

impl ApplicationHandler for MistilteinnApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.renderer.is_none() {
            let window = event_loop
                .create_window(
                    WindowAttributes::default()
                        .with_title("Mistilteinn")
                        .with_inner_size(winit::dpi::PhysicalSize::new(1280, 800)),
                )
                .expect("Failed to create window");

            log::info!("Window created (1280x800)");

            // Use tokio runtime to run the async wgpu initialization.
            // wgpu requires an async runtime for adapter/device requests.
            let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
            let renderer = rt.block_on(async {
                match Renderer::new(window).await {
                    Ok(renderer) => Some(renderer),
                    Err(e) => {
                        log::error!("Failed to initialize renderer: {}", e);
                        None
                    }
                }
            });
            self.renderer = renderer;

            // Load the default demo page through the pipeline
            if self.renderer.is_some() {
                self.load_page(
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
                );
                log::info!("First frame rendered — pipeline output (HTML→CSS→Layout→Render)");
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                log::info!("Window close requested, exiting");
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if size.width == 0 || size.height == 0 {
                    return; // Ignore spurious resize events
                }
                if let Some(ref mut renderer) = self.renderer {
                    renderer.resize(size.width, size.height);
                }
                // Trigger recompose with new view dimensions
                if let Some(ref mut page) = self.page {
                    page.view_width = size.width as f32;
                    page.view_height = size.height as f32;
                    // Rebuild layout positions with new view width
                    let mut text_renderer = TextRenderer::new();
                    crate::layout::compute_layout(
                        &mut page.layout_root,
                        page.view_width,
                        &mut text_renderer,
                    );
                }
                self.recompose();
                if let Some(ref renderer) = self.renderer {
                    renderer.window().request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(ref mut renderer) = self.renderer
                    && let Err(e) = renderer.render()
                {
                    log::error!("Render error: {:?}", e);
                }
                if let Some(ref renderer) = self.renderer {
                    renderer.window().request_redraw();
                }
            }
            _ => {}
        }
    }
}

/// Application entry point.
pub fn run() {
    let event_loop = EventLoop::new().expect("Failed to create event loop");
    let mut app = MistilteinnApp {
        renderer: None,
        page: None,
    };
    event_loop.run_app(&mut app).expect("Event loop failed");
}
