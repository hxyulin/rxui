//! Two ordinary winit/softbuffer windows with compositor-routed drag events.
//! Run on a Wayland session: cargo run -p draglet --example wayland_drag --features wayland
#[cfg(all(unix, not(target_os = "macos")))]
#[path = "support/mod.rs"]
mod support;
#[cfg(all(unix, not(target_os = "macos")))]
mod demo {
    use super::support::Canvas;
    use draglet::{CaptureState, wayland::WaylandAdapter};
    use std::time::{Duration, Instant};
    use winit::{
        application::ApplicationHandler,
        event::{ElementState, MouseButton, WindowEvent},
        event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
        window::WindowId,
    };
    #[derive(Default)]
    struct App {
        adapter: Option<WaylandAdapter>,
        windows: Vec<Canvas>,
    }
    impl ApplicationHandler for App {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.adapter.is_some() {
                return;
            }
            let mut adapter = WaylandAdapter::new(event_loop.owned_display_handle()).unwrap();
            for (title, color) in [("draglet blue", 0x003355bb), ("draglet green", 0x00339955)] {
                let canvas = Canvas::new(event_loop, title, color);
                adapter.register(&canvas.window).unwrap();
                eprintln!("REGISTER {:?}", canvas.window.id());
                self.windows.push(canvas);
            }
            self.adapter = Some(adapter);
        }
        fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
            match event {
                WindowEvent::CloseRequested => {
                    let adapter = self.adapter.as_mut().unwrap();
                    for canvas in &self.windows {
                        adapter.unregister(canvas.window.id());
                    }
                    event_loop.exit();
                }
                WindowEvent::RedrawRequested | WindowEvent::Resized(_) => {
                    if let Some(c) = self.windows.iter_mut().find(|c| c.window.id() == id) {
                        c.draw();
                    }
                }
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                    ..
                } => {
                    let adapter = self.adapter.as_mut().unwrap();
                    match adapter.begin_capture(id) {
                        Ok(()) => eprintln!("BEGIN {id:?}"),
                        Err(e) => eprintln!("BEGIN ERROR {e}"),
                    }
                }
                _ => {}
            }
        }
        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            if let Some(adapter) = &mut self.adapter {
                adapter.pump().unwrap();
                for event in adapter.take_events() {
                    eprintln!("EVENT {event:?}");
                }
                if let CaptureState::Released(id) | CaptureState::Lost(id) = adapter.capture_state()
                {
                    adapter.end_capture(id);
                }
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(8),
            ));
        }
    }
    pub fn run() {
        eprintln!(
            "Arrange the two windows side by side, then drag between their colored areas. Events appear here. Close either window to exit."
        );
        use winit::platform::wayland::EventLoopBuilderExtWayland;
        let event_loop = EventLoop::builder().with_wayland().build().unwrap();
        event_loop.run_app(&mut App::default()).unwrap();
    }
}
#[cfg(all(unix, not(target_os = "macos")))]
fn main() {
    demo::run();
}
#[cfg(not(all(unix, not(target_os = "macos"))))]
fn main() {
    eprintln!("Run this example in a Linux Wayland session.");
}
