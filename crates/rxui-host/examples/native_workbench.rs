//! Native entry point for the shared entity workbench.

use astrelis_core::geometry::Size;
use rxui_host::{
    WindowHostOptions,
    native::WindowAttributes,
    run_entity,
    workbench::{VIEWPORT_HEIGHT, VIEWPORT_WIDTH, Workbench},
};

fn main() -> Result<(), rxui_host::native::RuntimeError<std::io::Error>> {
    run_entity(
        |_| Workbench::new(),
        WindowHostOptions {
            window: WindowAttributes {
                title: "RXUI entity workbench".into(),
                inner_size: Some(Size::new(
                    f64::from(VIEWPORT_WIDTH),
                    f64::from(VIEWPORT_HEIGHT),
                )),
                ..WindowAttributes::default()
            },
            ..WindowHostOptions::default()
        },
    )
}
