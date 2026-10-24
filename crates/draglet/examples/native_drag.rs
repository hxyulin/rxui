//! AppKit/Win32 demo using ordinary winit input and native release snapshots.
#[cfg(any(
    all(target_os = "macos", feature = "appkit"),
    all(target_os = "windows", feature = "win32")
))]
#[path = "support/mod.rs"]
mod support;

#[cfg(any(
    all(target_os = "macos", feature = "appkit"),
    all(target_os = "windows", feature = "win32")
))]
mod demo {
    use super::support::Canvas;
    use draglet::{Adapter, CaptureState};
    use std::time::{Duration, Instant};
    use winit::{
        application::ApplicationHandler,
        event::{ElementState, MouseButton, WindowEvent},
        event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
        window::WindowId,
    };
    #[derive(Default)]
    struct App {
        adapter: Option<Adapter>,
        windows: Vec<Canvas>,
    }
    impl ApplicationHandler for App {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.adapter.is_some() {
                return;
            }
            let mut adapter = Adapter::new().expect("enable appkit on macOS or win32 on Windows");
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
                        adapter.unregister(canvas.window.id()).unwrap();
                    }
                    event_loop.exit();
                }
                WindowEvent::RedrawRequested | WindowEvent::Resized(_) => {
                    if let Some(canvas) = self.windows.iter_mut().find(|c| c.window.id() == id) {
                        canvas.draw();
                    }
                }
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                    ..
                } => match self.adapter.as_mut().unwrap().begin_capture(id) {
                    Ok(()) => eprintln!("BEGIN {id:?}"),
                    Err(e) => eprintln!("BEGIN ERROR {e}"),
                },
                _ => {}
            }
        }
        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            if let Some(adapter) = &mut self.adapter {
                for signal in adapter.take_signals() {
                    eprintln!("SIGNAL {signal:?}");
                }
                match adapter.capture_state() {
                    CaptureState::Held(_) => {
                        eprintln!("TARGET {:?}", adapter.target_at_pointer());
                    }
                    CaptureState::Released(id) => {
                        eprintln!("RELEASE {:?}", adapter.target_at_pointer());
                        adapter.end_capture(id);
                    }
                    CaptureState::Lost(id) => {
                        eprintln!("CANCELLED {id:?}");
                        adapter.end_capture(id);
                    }
                    CaptureState::Idle => {}
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
        EventLoop::new()
            .unwrap()
            .run_app(&mut App::default())
            .unwrap();
    }
}
#[cfg(any(
    all(target_os = "macos", feature = "appkit"),
    all(target_os = "windows", feature = "win32")
))]
fn main() {
    demo::run();
}
#[cfg(not(any(
    all(target_os = "macos", feature = "appkit"),
    all(target_os = "windows", feature = "win32")
)))]
fn main() {
    eprintln!("Enable appkit on macOS or win32 on Windows for this example.");
}
