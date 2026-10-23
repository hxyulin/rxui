use crate::{
    Error, Frame, FrameError, Framebuffer, FramebufferOptions, GraphicsContext, MeshRenderer,
    RenderTarget, Vertex, wgpu,
};

fn options(count: u32) -> FramebufferOptions {
    FramebufferOptions::new(64, 64).sample_count(count).usage(
        wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
    )
}

pub(crate) fn pixels(
    _graphics: &GraphicsContext,
    target: &mut Framebuffer,
    record: impl FnOnce(&mut Frame<'_, '_>),
) -> Vec<u8> {
    let mut frame = target.begin_frame().unwrap();
    record(&mut frame);
    frame.finish().unwrap();
    target.read_rgba8().unwrap()
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn framebuffer_contents_persist_only_after_submission() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let mut framebuffer = graphics.create_framebuffer(options(4)).unwrap();
        let mut frame = framebuffer.begin_frame().unwrap();
        assert!(matches!(
            frame.render_pass().load_color().begin(),
            Err(Error::UninitializedFramebuffer)
        ));
        drop(
            frame
                .render_pass()
                .clear_color(wgpu::Color::RED)
                .begin()
                .unwrap(),
        );
        drop(frame); // No submission: the clear must not initialize the resource.
        let mut frame = framebuffer.begin_frame().unwrap();
        assert!(matches!(
            frame.render_pass().load_color().begin(),
            Err(Error::UninitializedFramebuffer)
        ));
        drop(frame);
        let red = pixels(&graphics, &mut framebuffer, |frame| {
            let invalid = wgpu::Color {
                r: f64::NAN,
                ..wgpu::Color::GREEN
            };
            assert!(matches!(
                frame.render_pass().clear_color(invalid).begin(),
                Err(Error::InvalidClearColor)
            ));
            assert!(matches!(
                frame.render_pass().load_color().begin(),
                Err(Error::UninitializedFramebuffer)
            ));
            drop(
                frame
                    .render_pass()
                    .clear_color(wgpu::Color::RED)
                    .begin()
                    .unwrap(),
            );
            drop(frame.render_pass().load_color().begin().unwrap());
        });
        assert!(
            red.as_chunks::<4>()
                .0
                .iter()
                .all(|p| p == &[255, 0, 0, 255])
        );
        let preserved = pixels(&graphics, &mut framebuffer, |frame| {
            drop(frame.render_pass().load_color().begin().unwrap());
        });
        assert_eq!(
            red, preserved,
            "submitted contents survive into a new recording"
        );
        let mut frame = framebuffer.begin_frame().unwrap();
        drop(
            frame
                .render_pass()
                .clear_color(wgpu::Color::GREEN)
                .begin()
                .unwrap(),
        );
        drop(frame);
        let preserved = pixels(&graphics, &mut framebuffer, |frame| {
            drop(frame.render_pass().load_color().begin().unwrap());
        });
        assert_eq!(
            red, preserved,
            "dropping a clear preserves previously submitted contents"
        );

        let mut renderer = MeshRenderer::new(&graphics);
        let blue = graphics
            .create_mesh(
                &[
                    Vertex::new([-0.75, -0.75, 0.0], [0.0, 0.0, 1.0, 0.5]),
                    Vertex::new([0.75, -0.75, 0.0], [0.0, 0.0, 1.0, 0.5]),
                    Vertex::new([0.75, 0.75, 0.0], [0.0, 0.0, 1.0, 0.5]),
                ],
                &[0, 1, 2],
            )
            .unwrap();
        let blended = pixels(&graphics, &mut framebuffer, |frame| {
            let mut pass = frame.render_pass().load_color().begin().unwrap();
            renderer.draw(&mut pass, &blue).unwrap();
        });
        let point = 40 * 256 + 32 * 4;
        for (&actual, expected) in blended[point..point + 4].iter().zip([128_u8, 0, 128, 255]) {
            assert!(actual.abs_diff(expected) <= 1);
        }
        assert_eq!(&blended[..4], &[255, 0, 0, 255]);
        let output = framebuffer.color_texture().unwrap().clone();
        framebuffer.resize(64, 64).unwrap();
        framebuffer.set_sample_count(4).unwrap();
        assert_eq!(
            framebuffer.color_texture().unwrap(),
            &output,
            "unchanged settings reuse attachments"
        );
        assert!(matches!(
            framebuffer.set_sample_count(3),
            Err(Error::UnsupportedSampleCount { .. })
        ));
        assert_eq!(framebuffer.color_texture().unwrap(), &output);
        let max = graphics.device().limits().max_texture_dimension_2d;
        assert!(matches!(
            framebuffer.resize(max + 1, 64),
            Err(Error::InvalidTargetSize { .. })
        ));
        assert_eq!(framebuffer.color_texture().unwrap(), &output);
        framebuffer.set_sample_count(1).unwrap();
        assert_ne!(framebuffer.color_texture().unwrap(), &output);
        let mut frame = framebuffer.begin_frame().unwrap();
        assert!(matches!(
            frame.render_pass().load_color().begin(),
            Err(Error::UninitializedFramebuffer)
        ));
        drop(frame);
        framebuffer.resize(0, 64).unwrap();
        assert_eq!(
            framebuffer.begin_frame().unwrap_err(),
            FrameError::Suspended
        );
        assert!(matches!(
            framebuffer.color_texture(),
            Err(Error::TargetSuspended)
        ));
        assert!(matches!(
            framebuffer.color_view(),
            Err(Error::TargetSuspended)
        ));
        framebuffer.set_sample_count(4).unwrap();
        framebuffer.resize(64, 64).unwrap();
        assert_eq!(framebuffer.sample_count(), 4);
        assert_eq!(framebuffer.color_texture().unwrap().sample_count(), 1);
        let mut target: RenderTarget<'static> = framebuffer.into();
        assert!(target.as_surface().is_none());
        assert!(target.as_framebuffer().is_some());
        let mut frame = target.begin_frame().unwrap();
        assert_eq!(frame.sample_count(), 4);
        drop(frame.render_pass().begin().unwrap());
        frame.finish().unwrap();
        target.resize(0, 0).unwrap();
        assert_eq!(target.begin_frame().unwrap_err(), FrameError::Suspended);
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn framebuffer_passes_share_submission_and_validate_destinations() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let mut destination = graphics.create_framebuffer(options(1)).unwrap();
        let mut source = graphics.create_framebuffer(options(4)).unwrap();
        let shader = graphics
            .device()
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(
                    r#"
                @group(0) @binding(0) var image: texture_2d<f32>;
                @vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
                    let p = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
                    return vec4(p[i], 0.0, 1.0);
                }
                @fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
                    return textureLoad(image, vec2<i32>(p.xy), 0);
                }
            "#
                    .into(),
                ),
            });
        let pipeline = graphics
            .device()
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
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
                        format: destination.format(),
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
        let binding = graphics
            .device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source.color_view().unwrap()),
                }],
            });
        let mut frame = destination.begin_frame().unwrap();
        drop(
            frame
                .render_to(&mut source)
                .clear_color(wgpu::Color::RED)
                .begin()
                .unwrap(),
        );
        drop(frame);
        let mut frame = source.begin_frame().unwrap();
        assert!(matches!(
            frame.render_pass().load_color().begin(),
            Err(Error::UninitializedFramebuffer)
        ));
        drop(frame);
        let copied = pixels(&graphics, &mut destination, |frame| {
            drop(
                frame
                    .render_to(&mut source)
                    .clear_color(wgpu::Color::RED)
                    .begin()
                    .unwrap(),
            );
            drop(frame.render_to(&mut source).load_color().begin().unwrap());
            let mut pass = frame.render_pass().begin().unwrap();
            let raw = pass.as_wgpu();
            raw.set_pipeline(&pipeline);
            raw.set_bind_group(0, &binding, &[]);
            raw.draw(0..3, 0..1);
        });
        assert!(
            copied
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p == &[255, 0, 0, 255]),
            "the same submission resolves MSAA and samples its output"
        );
        let source_contents = pixels(&graphics, &mut source, |frame| {
            drop(frame.render_pass().load_color().begin().unwrap());
        });
        assert_eq!(copied, source_contents);
        // A queued clear of old attachments cannot initialize replacements.
        let mut frame = destination.begin_frame().unwrap();
        drop(frame.render_to(&mut source).begin().unwrap());
        source.resize(32, 32).unwrap();
        frame.finish().unwrap();
        let mut frame = source.begin_frame().unwrap();
        assert!(matches!(
            frame.render_pass().load_color().begin(),
            Err(Error::UninitializedFramebuffer)
        ));
        drop(frame);
        let mut suspended = graphics
            .create_framebuffer(FramebufferOptions::new(0, 0))
            .unwrap();
        let (device, queue) = graphics
            .adapter()
            .request_device(&Default::default())
            .await
            .unwrap();
        let other = GraphicsContext::from_wgpu(
            graphics.instance().clone(),
            graphics.adapter().clone(),
            device,
            queue,
        );
        let mut foreign = other
            .create_framebuffer(FramebufferOptions::new(64, 64))
            .unwrap();
        let mut frame = destination.begin_frame().unwrap();
        assert!(matches!(
            frame.render_to(&mut foreign).begin(),
            Err(Error::DeviceMismatch)
        ));
        assert!(matches!(
            frame.render_to(&mut suspended).begin(),
            Err(Error::TargetSuspended)
        ));
        drop(frame.render_pass().load_color().begin().unwrap());
        frame.finish().unwrap();
        for format in [
            wgpu::TextureFormat::Depth32Float,
            wgpu::TextureFormat::Bc1RgbaUnorm,
        ] {
            assert!(matches!(
                graphics.create_framebuffer(options(1).format(format)),
                Err(Error::UnsupportedColorFormat { .. })
            ));
        }
        for usage in [
            wgpu::TextureUsages::TEXTURE_BINDING,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TRANSIENT_ATTACHMENT,
        ] {
            assert!(matches!(
                graphics.create_framebuffer(options(1).usage(usage)),
                Err(Error::InvalidFramebufferUsage { .. })
            ));
        }
        assert!(matches!(
            graphics.create_framebuffer(FramebufferOptions::new(0, 0).sample_count(3)),
            Err(Error::UnsupportedSampleCount { .. })
        ));
        // Custom renderers can use integer color attachments; the float mesh shader rejects them.
        let mut integer = graphics
            .create_framebuffer(
                FramebufferOptions::new(64, 64).format(wgpu::TextureFormat::Rgba8Uint),
            )
            .unwrap();
        let mesh = graphics
            .create_mesh(
                &[
                    Vertex::new([0.0, 0.5, 0.0], [1.0; 4]),
                    Vertex::new([-0.5, -0.5, 0.0], [1.0; 4]),
                    Vertex::new([0.5, -0.5, 0.0], [1.0; 4]),
                ],
                &[0, 1, 2],
            )
            .unwrap();
        let mut renderer = MeshRenderer::new(&graphics);
        let mut frame = integer.begin_frame().unwrap();
        let mut pass = frame.render_pass().begin().unwrap();
        assert!(matches!(
            renderer.draw(&mut pass, &mesh),
            Err(Error::UnsupportedMeshFormat { .. })
        ));
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn rgba8_readback_unpads_rows_swizzles_bgra_and_rejects_unreadable_outputs() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC;
        let mut shapes = crate::ShapeRenderer::new(&graphics);
        for format in [
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Bgra8UnormSrgb,
        ] {
            // 70 texels per row pad to 512 bytes in the copy. Probes avoid antialiased edges.
            let mut target = graphics
                .create_framebuffer(FramebufferOptions::new(70, 8).format(format).usage(usage))
                .unwrap();
            assert!(matches!(
                target.read_rgba8(),
                Err(Error::UninitializedFramebuffer)
            ));
            shapes.prepare(&target.render_format()).unwrap();
            let mut frame = target.begin_frame().unwrap();
            {
                let mut pass = frame
                    .render_pass()
                    .clear_color(wgpu::Color::RED)
                    .begin()
                    .unwrap();
                shapes
                    .draw(
                        &mut pass,
                        crate::ShapeDraw::rect(
                            crate::Rect::new(30., 2., 40., 6.),
                            [0., 0., 1., 1.],
                        ),
                    )
                    .unwrap();
            }
            frame.finish().unwrap();
            let pixels = target.read_rgba8().unwrap();
            assert_eq!(pixels.len(), 70 * 8 * 4);
            let at = |x: usize, y: usize| &pixels[(y * 70 + x) * 4..][..4];
            for (x, y, expected) in [
                (0, 0, [255, 0, 0, 255]),
                (69, 0, [255, 0, 0, 255]),
                (20, 7, [255, 0, 0, 255]),
                (35, 4, [0, 0, 255, 255]),
                (60, 5, [0, 0, 255, 255]),
            ] {
                assert_eq!(at(x, y), expected, "{format:?} at {x},{y}");
            }
        }
        let mut unreadable = graphics
            .create_framebuffer(FramebufferOptions::new(4, 4))
            .unwrap();
        assert!(matches!(
            unreadable.read_rgba8(),
            Err(Error::UnsupportedReadback { .. })
        ));
        let mut float = graphics
            .create_framebuffer(
                FramebufferOptions::new(4, 4)
                    .format(wgpu::TextureFormat::Rgba16Float)
                    .usage(usage),
            )
            .unwrap();
        assert!(matches!(
            float.read_rgba8(),
            Err(Error::UnsupportedReadback { .. })
        ));
        let mut empty = graphics
            .create_framebuffer(FramebufferOptions::new(0, 4).usage(usage))
            .unwrap();
        assert!(matches!(empty.read_rgba8(), Err(Error::TargetSuspended)));
    });
}
