//! RXUI added to an ordinary winit/wgpu application. No RXUI desktop host or
//! managed graphics context is used. The app owns acquisition, recording and submission.
use rxui::{
    ExternalTexture, ImageAlpha, PointerEvent, UiPainter, UiRenderTarget, prelude::*, wgpu,
};
use std::{error::Error, sync::Arc};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

struct Panel {
    clicks: u32,
    preview: Image,
}
impl View for Panel {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column()
            .padding(24.)
            .gap(16.)
            .child(label("RXUI inside your wgpu app").font_size(28.))
            .child(label(
                "The app renders the texture below before RXUI samples it.",
            ))
            .child(
                image(self.preview.clone())
                    .width(320.)
                    .height(160.)
                    .fit(ImageFit::Stretch),
            )
            .child(
                row().opacity(0.85).child(
                    button(format!("Change application renderer ({})", self.clicks))
                        .on_click(cx.listener(|this, _, _| this.clicks += 1)),
                ),
            )
    }
}
struct State {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    runtime: Runtime,
    root: Entity<Panel>,
    ui: Ui<Panel>,
    painter: UiPainter,
    source: wgpu::Texture,
    external: ExternalTexture,
    pointer: [f32; 2],
}
impl State {
    async fn new(window: Arc<Window>) -> Result<Self, Box<dyn Error>> {
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await?;
        let (device, queue) = adapter.request_device(&Default::default()).await?;
        let size = window.inner_size();
        let config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("no supported surface configuration")?;
        surface.configure(&device, &config);
        let source = Self::source(&device, [size.width.max(1), size.height.max(1)]);
        let external = ExternalTexture::new(
            source.create_view(&Default::default()),
            [source.width(), source.height()],
        )?;
        let preview = Image::from_external_texture(external.clone(), ImageAlpha::Premultiplied);
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| cx.new(|_| Panel { clicks: 0, preview }));
        let ui = Ui::new(&mut runtime, root.clone())?;
        let mut painter = UiPainter::new(&device, &queue);
        painter.register_font(Arc::<[u8]>::from(
            include_bytes!("../tests/fonts/SourceSans3-Regular.otf").as_slice(),
        ))?;
        Ok(Self {
            window,
            surface,
            config,
            device,
            queue,
            runtime,
            root,
            ui,
            painter,
            source,
            external,
            pointer: [0.; 2],
        })
    }
    fn source(device: &wgpu::Device, size: [u32; 2]) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("application preview"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    }
    fn resize(&mut self, size: winit::dpi::PhysicalSize<u32>) -> Result<(), Box<dyn Error>> {
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
        self.source = Self::source(&self.device, [size.width, size.height]);
        self.external.replace(
            self.source.create_view(&Default::default()),
            [size.width, size.height],
        )?;
        self.window.request_redraw();
        Ok(())
    }
    fn draw(&mut self) -> Result<(), Box<dyn Error>> {
        let size = self.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        self.runtime.flush()?;
        let scale = self.window.scale_factor() as f32;
        self.ui.prepare(
            &mut self.runtime,
            [size.width as f32 / scale, size.height as f32 / scale],
            &mut self.painter,
        )?;
        self.painter.prepare(
            &self.ui,
            &rxui::RenderFormat::color(self.config.format, 1),
            scale,
        )?;
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                self.window.request_redraw();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("surface acquisition failed".into());
            }
        };
        let output = frame.texture.create_view(&Default::default());
        let source = self.source.create_view(&Default::default());
        let clicks = self.runtime.update(|cx| self.root.read(cx).clicks);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        // Replace this pass with your engine, chart, compute or custom renderer.
        {
            let colors = [Some(wgpu::RenderPassColorAttachment {
                view: &source,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.08,
                        g: 0.15 + (clicks % 4) as f64 * 0.15,
                        b: 0.35,
                        a: 1.,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })];
            drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &colors,
                ..Default::default()
            }));
        }
        let mut destination =
            UiRenderTarget::new(&output, self.config.format, [size.width, size.height]);
        destination.load = wgpu::LoadOp::Clear(wgpu::Color {
            r: 0.025,
            g: 0.03,
            b: 0.045,
            a: 1.,
        });
        let resources = self
            .painter
            .record(&self.ui, &mut encoder, &destination, scale)?;
        self.queue.submit([encoder.finish()]);
        resources.after_submit(&self.queue);
        self.queue.present(frame);
        Ok(())
    }
    fn pointer(&mut self, event: PointerEvent) -> Result<(), Box<dyn Error>> {
        self.ui.pointer(&mut self.runtime, event)?;
        self.runtime.flush()?;
        self.window.request_redraw();
        Ok(())
    }
}
#[derive(Default)]
struct App(Option<State>);
impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.0.is_none() {
            let result = (|| {
                let window =
                    Arc::new(event_loop.create_window(
                        Window::default_attributes().with_title("RXUI + raw wgpu"),
                    )?);
                let state = pollster::block_on(State::new(window))?;
                state.window.request_redraw();
                Ok::<_, Box<dyn Error>>(state)
            })();
            match result {
                Ok(state) => self.0 = Some(state),
                Err(error) => {
                    eprintln!("{error}");
                    event_loop.exit();
                }
            }
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(state) = &mut self.0 else {
            return;
        };
        let result = match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                Ok(())
            }
            WindowEvent::Resized(size) => state.resize(size),
            WindowEvent::ScaleFactorChanged { .. } => state.resize(state.window.inner_size()),
            WindowEvent::RedrawRequested => state.draw(),
            WindowEvent::CursorMoved { position, .. } => {
                let position = position.to_logical::<f32>(state.window.scale_factor());
                state.pointer = [position.x, position.y];
                state.pointer(PointerEvent::Moved(state.pointer))
            }
            WindowEvent::MouseInput {
                state: button_state,
                button: MouseButton::Left,
                ..
            } => {
                let event = if button_state == ElementState::Pressed {
                    PointerEvent::Down {
                        position: state.pointer,
                        button: rxui::PointerButton::Primary,
                        modifiers: Default::default(),
                    }
                } else {
                    PointerEvent::Up {
                        position: state.pointer,
                        button: rxui::PointerButton::Primary,
                        modifiers: Default::default(),
                    }
                };
                state.pointer(event)
            }
            _ => Ok(()),
        };
        if let Err(error) = result {
            eprintln!("{error}");
            event_loop.exit();
        }
    }
}
fn main() -> Result<(), Box<dyn Error>> {
    EventLoop::new()?.run_app(&mut App::default())?;
    Ok(())
}
