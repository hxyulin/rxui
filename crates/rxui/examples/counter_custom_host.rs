//! Standalone desktop example: copy this file into a binary using rxui's native feature.
//! Click controls, Tab/Shift-Tab changes focus, Enter/Space activates, Escape closes.
//! Background work uses a native completion message; there is no hidden executor.
use rxui::astrelis_winit::{
    AppContext as NativeContext, Handler, PrepareAction, Runner, SurfaceSettings, WindowInfo,
    astrelis::{Frame, wgpu},
    winit::{
        dpi::LogicalSize,
        event::{ElementState, MouseButton, WindowEvent},
        event_loop::EventLoopProxy,
        keyboard::{Key, ModifiersState, NamedKey},
        window::{Window, WindowId},
    },
};
use rxui::{PointerEvent, UiPainter, prelude::*};
use std::{error::Error, thread, time::Duration};

struct Completion {
    request: u64,
    value: u64,
}
struct Counter {
    count: i32,
    items: Vec<u32>,
    next_item: u32,
    selected: Option<u32>,
    request: u64,
    loading: bool,
    loaded: Option<u64>,
    proxy: EventLoopProxy<Completion>,
}
impl Counter {
    fn load(&mut self, _: &ClickEvent, _: &mut Context<'_, Self>) {
        self.request = self.request.checked_add(1).expect("request IDs exhausted");
        self.loading = true;
        let request = self.request;
        let proxy = self.proxy.clone();
        // Simulates a blocking service call, explicitly off the UI thread. A future
        // executor can deliver the same owned completion via its task/proxy adapter.
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(750));
            let _ = proxy.send_event(Completion {
                request,
                value: request * 100,
            });
        });
    }
}
impl View for Counter {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let status = if self.loading {
            "Loading in the background…".to_owned()
        } else {
            format!("Background result: {:?}", self.loaded)
        };
        column()
            .fill_width()
            .padding(24.)
            .gap(16.)
            .child(label("RXUI declarative core").font_size(28.))
            .child(
                label("Click a button, or use Tab and Enter. Resize to exercise Taffy layout.")
                    .color([0.65, 0.72, 0.84, 1.]),
            )
            .child(label(format!("Counter: {}", self.count)).font_size(22.))
            .child(
                row()
                    .gap(12.)
                    .child(
                        button("Decrease")
                            .key("decrease")
                            .on_click(cx.listener(|this, _, _| this.count -= 1)),
                    )
                    .child(
                        button("Increase")
                            .key("increase")
                            .on_click(cx.listener(|this, _, _| this.count += 1)),
                    ),
            )
            .child(
                row()
                    .gap(12.)
                    .child(
                        button("Reverse items")
                            .key("reverse")
                            .on_click(cx.listener(|this, _, _| this.items.reverse())),
                    )
                    .child(
                        button("Add item")
                            .key("add")
                            .on_click(cx.listener(|this, _, _| {
                                this.items.push(this.next_item);
                                this.next_item += 1;
                            })),
                    )
                    .child(
                        button("Remove first")
                            .key("remove")
                            .disabled(self.items.is_empty())
                            .on_click(cx.listener(|this, _, _| {
                                if !this.items.is_empty() {
                                    this.items.remove(0);
                                }
                            })),
                    ),
            )
            .child(row().gap(12.).children(self.items.iter().map(|item| {
                let id = *item;
                button(format!("Item {id}"))
                    .key(id)
                    .background(if self.selected == Some(id) {
                        [0.08, 0.3, 0.25, 1.]
                    } else {
                        [0.10, 0.16, 0.24, 1.]
                    })
                    .on_click(cx.listener(move |this, _, _| this.selected = Some(id)))
            })))
            .child(label(
                "Keys preserve focus when items reorder; removed items lose capture.",
            ))
            .child(
                row()
                    .gap(12.)
                    .child(
                        button("Start background job")
                            .key("load")
                            .on_click(cx.listener(Self::load)),
                    )
                    .child(
                        button("Discard pending result")
                            .key("cancel")
                            .disabled(!self.loading)
                            .on_click(cx.listener(|this, _, _| {
                                this.request += 1;
                                this.loading = false;
                            })),
                    ),
            )
            .child(label(status))
    }
}

struct App {
    runtime: Runtime,
    ui: Option<Ui<Counter>>,
    painter: Option<UiPainter>,
    window: Option<WindowId>,
    cursor: [f32; 2],
    modifiers: ModifiersState,
}
impl Default for App {
    fn default() -> Self {
        Self {
            runtime: Runtime::new(),
            ui: None,
            painter: None,
            window: None,
            cursor: [0.; 2],
            modifiers: ModifiersState::default(),
        }
    }
}
impl Handler for App {
    type Message = Completion;
    type Error = Box<dyn Error>;
    fn resumed(&mut self, cx: &mut NativeContext<'_, Completion>) -> Result<(), Self::Error> {
        if self.window.is_none() {
            if self.ui.is_none() {
                let proxy = cx.proxy();
                let root = self.runtime.update(|cx| {
                    cx.new(|_| Counter {
                        count: 0,
                        items: vec![1, 2, 3],
                        next_item: 4,
                        selected: None,
                        request: 0,
                        loading: false,
                        loaded: None,
                        proxy,
                    })
                });
                self.ui = Some(Ui::new(&mut self.runtime, root)?);
            }
            self.window = Some(
                cx.create_window(
                    Window::default_attributes()
                        .with_title("RXUI — builders, keys and background completion")
                        .with_inner_size(LogicalSize::new(820., 600.)),
                    SurfaceSettings::new(),
                )?,
            );
        }
        Ok(())
    }
    fn window_created(
        &mut self,
        cx: &mut NativeContext<'_, Completion>,
        id: WindowId,
    ) -> Result<(), Self::Error> {
        let mut painter = UiPainter::new(cx.window(id).unwrap().graphics());
        // Discovery is explicit and occurs once, before normal frame preparation.
        if painter.fonts_mut().load_system_fonts().is_empty() {
            return Err("No system fonts found; load an application font explicitly".into());
        }
        self.painter = Some(painter);
        Ok(())
    }
    fn window_event(
        &mut self,
        cx: &mut NativeContext<'_, Completion>,
        id: WindowId,
        event: WindowEvent,
    ) -> Result<(), Self::Error> {
        if self.window != Some(id) {
            return Ok(());
        }
        let ui = self.ui.as_mut().unwrap();
        let mut changed = false;
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let scale = cx.window(id).unwrap().metrics().scale_factor();
                self.cursor = [(position.x / scale) as f32, (position.y / scale) as f32];
                changed = ui.pointer(&mut self.runtime, PointerEvent::Moved(self.cursor))?;
            }
            WindowEvent::CursorLeft { .. } => {
                changed = ui.pointer(&mut self.runtime, PointerEvent::Left)?
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let input = if state == ElementState::Pressed {
                    PointerEvent::Pressed(self.cursor)
                } else {
                    PointerEvent::Released(self.cursor)
                };
                changed = ui.pointer(&mut self.runtime, input)?;
            }
            WindowEvent::Focused(false) => {
                changed = ui.pointer(&mut self.runtime, PointerEvent::Cancelled)?
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed && !event.repeat =>
            {
                match event.logical_key {
                    Key::Named(NamedKey::Tab) => {
                        changed = ui.focus_next(self.modifiers.shift_key())
                    }
                    Key::Named(NamedKey::Enter | NamedKey::Space) => {
                        changed = ui.activate_focused(&mut self.runtime)?
                    }
                    Key::Named(NamedKey::Escape) => cx.close_window(id)?,
                    _ => {}
                }
            }
            _ => {}
        }
        self.runtime.flush()?;
        if changed {
            cx.request_redraw(id)?;
        }
        Ok(())
    }
    fn user_event(
        &mut self,
        cx: &mut NativeContext<'_, Completion>,
        message: Completion,
    ) -> Result<(), Self::Error> {
        let Some(ui) = &self.ui else {
            return Ok(());
        };
        // Resolve the weak UI target on the UI thread. Superseded/cancelled results
        // have no mutation effect and never replace newer application data.
        let owner = ui.entity().downgrade();
        let changed = self.runtime.update(|cx| {
            let Some(entity) = owner.upgrade() else {
                return false;
            };
            let accept = {
                let state = entity.read(cx);
                state.loading && state.request == message.request
            };
            if accept {
                entity.update(cx, |this, _| {
                    this.loaded = Some(message.value);
                    this.loading = false;
                });
            }
            accept
        });
        self.runtime.flush()?;
        if changed && let Some(id) = self.window {
            cx.request_redraw(id)?;
        }
        Ok(())
    }
    fn prepare(
        &mut self,
        cx: &mut NativeContext<'_, Completion>,
        id: WindowId,
    ) -> Result<PrepareAction, Self::Error> {
        let window = cx.window(id).unwrap();
        let metrics = window.metrics();
        let size = metrics.logical_size();
        let painter = self.painter.as_mut().unwrap();
        let ui = self.ui.as_mut().unwrap();
        self.runtime.flush()?;
        ui.prepare(
            &mut self.runtime,
            [size.width as f32, size.height as f32],
            painter,
        )?;
        painter.prepare(
            ui,
            window.render_format().unwrap(),
            metrics.scale_factor() as f32,
        )?;
        Ok(PrepareAction::Render)
    }
    fn render(
        &mut self,
        window: WindowInfo<'_>,
        frame: &mut Frame<'_, 'static>,
    ) -> Result<(), Self::Error> {
        self.painter.as_mut().unwrap().compose(
            self.ui.as_ref().unwrap(),
            frame,
            window.metrics().scale_factor() as f32,
            |frame, ui| {
                let mut pass = frame
                    .render_pass()
                    .clear_color(wgpu::Color {
                        r: 0.025,
                        g: 0.035,
                        b: 0.055,
                        a: 1.,
                    })
                    .begin()?;
                ui.paint(&mut pass)
            },
        )?;
        Ok(())
    }
    fn window_closed(
        &mut self,
        _: &mut NativeContext<'_, Completion>,
        id: WindowId,
    ) -> Result<(), Self::Error> {
        if self.window == Some(id) {
            self.window = None;
            self.ui = None;
            self.painter = None;
        }
        Ok(())
    }
}
fn main() -> Result<(), Box<dyn Error>> {
    Runner::new()?.run(&mut App::default())?;
    Ok(())
}
