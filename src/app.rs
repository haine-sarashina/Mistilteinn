use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::WindowAttributes,
};

/// Main application struct implementing winit's ApplicationHandler trait.
pub struct MistilteinnApp {
    window: Option<winit::window::Window>,
}

impl ApplicationHandler for MistilteinnApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let window = event_loop
                .create_window(
                    WindowAttributes::default()
                        .with_title("Mistilteinn")
                        .with_inner_size(winit::dpi::PhysicalSize::new(1280, 800)),
                )
                .expect("Failed to create window");

            log::info!("Window created");
            self.window = Some(window);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        if let WindowEvent::CloseRequested = event {
            log::info!("Window close requested, exiting");
            event_loop.exit();
        }
    }
}

/// Application entry point.
///
/// Creates the winit event loop and drives the main window.
pub fn run() {
    let event_loop = EventLoop::new().expect("Failed to create event loop");
    let mut app = MistilteinnApp { window: None };
    event_loop.run_app(&mut app).expect("Event loop failed");
}
