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
                        // Set up demo rectangles: red, green, blue, yellow
                        let (w, h) = (1280.0, 800.0);
                        let rects = vec![
                            layout_to_clip(100.0, 100.0, 400.0, 300.0, w, h),
                            layout_to_clip(600.0, 100.0, 300.0, 200.0, w, h),
                            layout_to_clip(200.0, 500.0, 500.0, 200.0, w, h),
                            layout_to_clip(750.0, 400.0, 200.0, 250.0, w, h),
                        ];
                        let colors = vec![
                            ColorF { r: 1.0, g: 0.0, b: 0.0, a: 1.0 },
                            ColorF { r: 0.0, g: 1.0, b: 0.0, a: 1.0 },
                            ColorF { r: 0.0, g: 0.0, b: 1.0, a: 1.0 },
                            ColorF { r: 1.0, g: 1.0, b: 0.0, a: 1.0 },
                        ];
                        renderer.set_rects(&rects, &colors);

                        if let Err(e) = renderer.render() {
                            log::error!("Initial render failed: {:?}", e);
                        } else {
                            log::info!("First frame rendered — 4 colored rectangles");
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
