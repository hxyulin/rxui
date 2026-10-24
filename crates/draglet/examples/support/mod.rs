//! Rendering used only by the examples; draglet has no rendering dependency.
use std::{num::NonZeroU32, sync::Arc};
use winit::{event_loop::ActiveEventLoop, window::Window};

pub struct Canvas {
    pub window: Arc<Window>,
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
    _context: softbuffer::Context<Arc<Window>>,
    color: u32,
}

impl Canvas {
    pub fn new(event_loop: &ActiveEventLoop, title: &str, color: u32) -> Self {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title(title)
                        .with_inner_size(winit::dpi::LogicalSize::new(300., 240.)),
                )
                .expect("create demo window"),
        );
        let context = softbuffer::Context::new(window.clone()).expect("create software context");
        let surface = softbuffer::Surface::new(&context, window.clone()).expect("create surface");
        window.request_redraw();
        Self {
            window,
            surface,
            _context: context,
            color,
        }
    }

    pub fn draw(&mut self) {
        let size = self.window.inner_size();
        if let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) {
            self.surface.resize(w, h).expect("resize surface");
            let mut buffer = self.surface.buffer_mut().expect("acquire buffer");
            buffer.fill(self.color);
            self.window.pre_present_notify();
            buffer.present().expect("present buffer");
        }
    }
}
