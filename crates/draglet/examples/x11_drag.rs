//! Standalone X11 demo: passive input bridging and cross-window targeting.
#[cfg(all(unix, not(target_os = "macos")))]
#[path = "support/mod.rs"]
mod support;

#[cfg(all(unix, not(target_os = "macos")))]
mod demo {
    use super::support::Canvas;
    use draglet::x11::{X11Adapter, X11Input};
    use std::time::{Duration, Instant};
    use winit::{
        application::ApplicationHandler,
        event::WindowEvent,
        event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
        platform::x11::EventLoopBuilderExtX11,
        window::WindowId,
    };

    #[derive(Default)]
    struct App {
        adapter: Option<X11Adapter>,
        windows: Vec<Canvas>,
        source: Option<WindowId>,
    }
    impl ApplicationHandler for App {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.adapter.is_some() {
                return;
            }
            // Passing DISPLAY explicitly also permits an isolated Xvfb display
            // launched from a shell whose parent session happens to be Wayland.
            let display = std::env::var("DISPLAY").expect("set DISPLAY to a standalone X server");
            let mut adapter = X11Adapter::connect(Some(&display)).expect("standalone X11 backend");
            for (title, color) in [("draglet blue", 0x003355bb), ("draglet green", 0x00339955)] {
                let canvas = Canvas::new(event_loop, title, color);
                adapter.register(&canvas.window).unwrap();
                // Arm before any primary press; winit will not deliver this input.
                adapter.arm(canvas.window.id()).unwrap();
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
                        adapter.unregister(canvas.window.id()).unwrap();
                    }
                    event_loop.exit();
                }
                WindowEvent::RedrawRequested | WindowEvent::Resized(_) => {
                    if let Some(canvas) = self.windows.iter_mut().find(|c| c.window.id() == id) {
                        canvas.draw();
                    }
                }
                _ => {}
            }
        }
        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            if let Some(adapter) = &mut self.adapter {
                while let Some(input) = adapter.poll_input().unwrap() {
                    // A real UI should bridge these primary events into its input
                    // system, including clicks that never become drags.
                    match input {
                        X11Input::Pressed { window, .. } => {
                            // A quick click can already have a queued release;
                            // that is not a live gesture we can take over.
                            match adapter.acquire_capture(window) {
                                Ok(()) => {
                                    self.source = Some(window);
                                    eprintln!("BEGIN {window:?}");
                                }
                                Err(e) => eprintln!("BEGIN ERROR {e}"),
                            }
                        }
                        X11Input::Moved { .. } if self.source.is_some() => {
                            eprintln!("TARGET {:?}", adapter.target_at_pointer());
                        }
                        X11Input::Released { window, .. } => {
                            // Query before releasing capture. Application policy
                            // decides whether this registered target accepts a drop.
                            eprintln!("RELEASE {:?}", adapter.target_at_pointer());
                            adapter.release_capture(window);
                            self.source = None;
                        }
                        X11Input::Cancelled { window } => {
                            eprintln!("CANCELLED {window:?}");
                            adapter.release_capture(window);
                            self.source = None;
                        }
                        _ => {}
                    }
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
        EventLoop::builder()
            .with_x11()
            .build()
            .unwrap()
            .run_app(&mut App::default())
            .unwrap();
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn main() {
    demo::run();
}
#[cfg(not(all(unix, not(target_os = "macos"))))]
fn main() {
    eprintln!("Run this example on a standalone Linux X11 server.");
}
