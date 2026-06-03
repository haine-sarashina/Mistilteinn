use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::WindowAttributes,
};

use crate::render::{ColorF, Renderer, layout_to_clip};

/// Main application struct implementing winit's ApplicationHandler trait.
pub struct MistilteinnApp {
    renderer: Option<Renderer>,
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
                    Ok(mut renderer) => {
                        // Build page through the full pipeline: HTML → CSS → Layout → Render
                        let (w, h) = (1280.0, 800.0);
                        let page = crate::page::Page::new(
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
                            w,
                            h,
                        );

                        // Collect render rectangles from layout tree and convert to clip space
                        let rects = page.collect_rects();
                        let clip_rects: Vec<_> = rects
                            .iter()
                            .take(4)
                            .filter_map(|(r, c)| {
                                if r.width > 0.0 && r.height > 0.0 {
                                    Some((layout_to_clip(r.x, r.y, r.width, r.height, w, h), c))
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
                                    ColorF { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }
                                }
                            })
                            .collect();

                        renderer.set_rects(&render_rects, &render_colors);

                        if let Err(e) = renderer.render() {
                            log::error!("Initial render failed: {:?}", e);
                        } else {
                            log::info!("First frame rendered — pipeline output (HTML→CSS→Layout→Render)");
                        }

                        Some(renderer)
                    }
                    Err(e) => {
                        log::error!("Failed to initialize renderer: {}", e);
                        None
                    }
                }
            });
            self.renderer = renderer;
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
                if let Some(ref mut renderer) = self.renderer {
                    renderer.resize(size.width, size.height);
                }
                if let Some(ref renderer) = self.renderer {
                    renderer.window().request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(ref mut renderer) = self.renderer {
                    if let Err(e) = renderer.render() {
                        log::error!("Render error: {:?}", e);
                    }
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
    let mut app = MistilteinnApp { renderer: None };
    event_loop.run_app(&mut app).expect("Event loop failed");
}
