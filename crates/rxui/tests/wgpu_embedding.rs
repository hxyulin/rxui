//! External integration regression: deliberately uses no managed drawing/host types.
#![cfg(feature = "rendering")]
use rxui::{
    CustomElement, CustomPrepare, ExternalTexture, ImageAlpha, UiPainter, UiRenderTarget,
    prelude::*, wgpu,
};

#[derive(PartialEq)]
struct Blue;
impl CustomElement for Blue {
    type State = Option<wgpu::RenderPipeline>;
    fn prepare(
        &self,
        state: &mut Self::State,
        _: &rxui::ElementInfo<'_>,
        cx: &mut CustomPrepare<'_>,
    ) -> Result<(), rxui::UiError> {
        if state.is_none() {
            let shader = cx
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("application blue triangle"),
                    source: wgpu::ShaderSource::Wgsl(
                        r#"
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let p = array<vec2f, 3>(vec2f(-1., -1.), vec2f(3., -1.), vec2f(-1., 3.));
    return vec4f(p[i], 0., 1.);
}
@fragment fn fs() -> @location(0) vec4f { return vec4f(0., 0., 1., 1.); }
"#
                        .into(),
                    ),
                });
            *state = Some(
                cx.device
                    .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some("application pipeline"),
                        layout: None,
                        vertex: wgpu::VertexState {
                            module: &shader,
                            entry_point: Some("vs"),
                            compilation_options: Default::default(),
                            buffers: &[],
                        },
                        fragment: Some(wgpu::FragmentState {
                            module: &shader,
                            entry_point: Some("fs"),
                            compilation_options: Default::default(),
                            targets: &[Some(wgpu::ColorTargetState {
                                format: cx.format.colors[0].unwrap(),
                                blend: None,
                                write_mask: wgpu::ColorWrites::ALL,
                            })],
                        }),
                        primitive: Default::default(),
                        depth_stencil: None,
                        multisample: wgpu::MultisampleState {
                            count: cx.format.sample_count,
                            ..Default::default()
                        },
                        multiview_mask: None,
                        cache: None,
                    }),
            );
        }
        Ok(())
    }
    fn paint(
        &self,
        state: &Self::State,
        element: &rxui::ElementInfo<'_>,
        paint: &mut rxui::drawing::PaintSession<'_, '_>,
    ) -> Result<(), rxui::UiError> {
        let b = element.content_bounds;
        let pass = paint.wgpu();
        pass.set_viewport(b.x, b.y, b.width, b.height, 0., 1.);
        pass.set_pipeline(state.as_ref().unwrap());
        pass.draw(0..3, 0..1);
        Ok(())
    }
}
struct Scene(Image);
impl View for Scene {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        row()
            .width(32.)
            .height(16.)
            .opacity(0.5)
            .child(rxui::custom(Blue).width(16.).height(16.))
            .child(image(self.0.clone()).width(16.).height(16.))
    }
}
fn texture(device: &wgpu::Device, size: [u32; 2], usage: wgpu::TextureUsages) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("application texture"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage,
        view_formats: &[],
    })
}
fn upload(queue: &wgpu::Queue, texture: &wgpu::Texture, color: [u8; 4]) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: Default::default(),
            aspect: wgpu::TextureAspect::All,
        },
        &color.repeat((texture.width() * texture.height()) as usize),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(texture.width() * 4),
            rows_per_image: Some(texture.height()),
        },
        texture.size(),
    );
}
fn readback(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
) -> wgpu::Buffer {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 256 * 16,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: Default::default(),
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(16),
            },
        },
        texture.size(),
    );
    buffer
}
fn pixels(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Vec<u8> {
    let (tx, rx) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(20)),
        })
        .unwrap();
    rx.recv().unwrap().unwrap();
    let result = buffer.slice(..).get_mapped_range().unwrap().to_vec();
    buffer.unmap();
    result
}
#[test]
#[ignore = "requires a GPU"]
fn application_device_custom_pipeline_live_texture_opacity_and_delayed_submission() {
    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance.request_adapter(&Default::default()).await.unwrap();
        let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
        let errors = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut painter = UiPainter::new(&device, &queue);
        let sampling = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
        let green = texture(&device, [1, 1], sampling);
        upload(&queue, &green, [0, 255, 0, 255]);
        let live = ExternalTexture::new(green.create_view(&Default::default()), [1, 1]).unwrap();
        let source = Image::from_external_texture(live.clone(), ImageAlpha::Straight);
        let identity = source.id();
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| cx.new(|_| Scene(source.clone())));
        let mut ui = Ui::new(&mut runtime, root).unwrap();
        let mut recordings = Vec::new();
        let mut buffers = Vec::new();
        for iteration in 0..2 {
            if iteration == 1 {
                let yellow = texture(&device, [2, 2], sampling);
                upload(&queue, &yellow, [255, 255, 0, 255]);
                live.replace(yellow.create_view(&Default::default()), [2, 2])
                    .unwrap();
                assert_eq!(identity, source.id());
                assert_eq!(source.pixel_size(), Some([2, 2]));
            }
            let output = texture(
                &device,
                [32, 16],
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            );
            let output = if iteration == 0 {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size: output.size(),
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Bgra8Unorm,
                    usage: output.usage(),
                    view_formats: &[],
                })
            } else {
                output
            };
            let view = output.create_view(&Default::default());
            let mut encoder = device.create_command_encoder(&Default::default());
            // Existing application content must survive RXUI's default Load operation.
            {
                let colors = [Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::RED),
                        store: wgpu::StoreOp::Store,
                    },
                })];
                drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &colors,
                    ..Default::default()
                }));
            }
            let target = UiRenderTarget::new(&view, output.format(), [32, 16]);
            ui.prepare(&mut runtime, [32., 16.], &mut painter).unwrap();
            painter.prepare(&ui, &target.render_format(), 1.).unwrap();
            let resources = painter.record(&ui, &mut encoder, &target, 1.).unwrap();
            buffers.push(readback(&device, &mut encoder, &output));
            recordings.push((encoder.finish(), resources));
        }
        // Both recordings exist before either submits. Storage and glyph leases must stay isolated.
        for (commands, resources) in recordings {
            queue.submit([commands]);
            resources.after_submit(&queue);
        }
        for (i, buffer) in buffers.iter().enumerate() {
            let bytes = pixels(&device, buffer);
            let blue = &bytes[8 * 256 + 8 * 4..][..4];
            assert!(
                blue[0].abs_diff(128) <= 1 && blue[2].abs_diff(128) <= 1,
                "custom renderer: {blue:?}"
            );
            let image = &bytes[8 * 256 + 24 * 4..][..4];
            let expected = if i == 0 {
                [0, 128, 128, 255]
            } else {
                [255, 128, 0, 255]
            };
            assert!(
                image.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1),
                "image {i}: {image:?}"
            );
        }
        live.suspend();
        assert_eq!(source.pixel_size(), None);
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU"]
fn text_msaa_discard_and_renderer_drop_before_submission() {
    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance.request_adapter(&Default::default()).await.unwrap();
        let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
        let errors = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut painter = UiPainter::new(&device, &queue);
        painter
            .register_font(std::sync::Arc::<[u8]>::from(
                include_bytes!("fonts/SourceSans3-Regular.otf").as_slice(),
            ))
            .unwrap();
        struct Text;
        impl View for Text {
            fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
                column()
                    .width(32.)
                    .height(16.)
                    .opacity(0.5)
                    .child(label("Hi").font_size(12.).color([1.; 4]))
            }
        }
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| cx.new(|_| Text));
        let mut ui = Ui::new(&mut runtime, root).unwrap();
        let output = texture(
            &device,
            [32, 16],
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let msaa = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: output.size(),
            mip_level_count: 1,
            sample_count: 4,
            dimension: wgpu::TextureDimension::D2,
            format: output.format(),
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = msaa.create_view(&Default::default());
        let resolve = output.create_view(&Default::default());
        let mut target = UiRenderTarget::new(&view, output.format(), [32, 16]);
        target.resolve_target = Some(&resolve);
        target.load = wgpu::LoadOp::Clear(wgpu::Color::BLACK);
        ui.prepare(&mut runtime, [32., 16.], &mut painter).unwrap();
        painter.prepare(&ui, &target.render_format(), 1.).unwrap();
        // An abandoned recording must not poison the next composition or resource leases.
        let mut discarded = device.create_command_encoder(&Default::default());
        let resources = painter.record(&ui, &mut discarded, &target, 1.).unwrap();
        drop(discarded);
        drop(resources);
        let mut encoder = device.create_command_encoder(&Default::default());
        let resources = painter.record(&ui, &mut encoder, &target, 1.).unwrap();
        let buffer = readback(&device, &mut encoder, &output);
        painter.forget(&ui);
        drop(ui);
        drop(painter);
        queue.submit([encoder.finish()]);
        resources.after_submit(&queue);
        let bytes = pixels(&device, &buffer);
        assert!(
            (0..16).any(|y| (0..32).any(|x| bytes[y * 256 + x * 4] > 20)),
            "prepared text disappeared after dropping the renderer"
        );
        assert!(errors.pop().await.is_none());
    });
}
