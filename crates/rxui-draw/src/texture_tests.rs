use super::*;
use crate::framebuffer::tests::pixels;
use crate::{Framebuffer, FramebufferOptions, MeshRenderer, TextureOptions, Vertex};

fn output(g: &GraphicsContext, count: u32) -> Framebuffer {
    g.create_framebuffer(FramebufferOptions::new(64, 64).sample_count(count).usage(
        wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
    ))
    .unwrap()
}
fn pixel(bytes: &[u8], x: usize, y: usize) -> [u8; 4] {
    bytes[(y * 64 + x) * 4..(y * 64 + x + 1) * 4]
        .try_into()
        .unwrap()
}
fn near(actual: [u8; 4], expected: [u8; 4]) {
    for (a, e) in actual.into_iter().zip(expected) {
        assert!(a.abs_diff(e) <= 2, "{actual:?} != {expected:?}");
    }
}
fn image(g: &GraphicsContext, w: u32, h: u32, bytes: &[u8]) -> crate::Texture {
    let t = g
        .create_texture(TextureOptions::new(w, h).format(wgpu::TextureFormat::Rgba8Unorm))
        .unwrap();
    t.write(bytes).unwrap();
    t
}
#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn uploads_validate_and_regions_preserve_other_texels() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let scope = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        for (width, height) in [(0, 2), (u32::MAX, 2)] {
            assert!(matches!(
                g.create_texture(TextureOptions::new(width, height)),
                Err(Error::InvalidTextureSize { .. })
            ));
        }
        for format in [
            wgpu::TextureFormat::Depth24Plus,
            wgpu::TextureFormat::Bc1RgbaUnorm,
        ] {
            assert!(matches!(
                g.create_texture(TextureOptions::new(2, 2).format(format)),
                Err(Error::UnsupportedTextureFormat { .. })
            ));
        }
        assert!(matches!(
            g.create_texture(TextureOptions::new(2, 2).usage(wgpu::TextureUsages::empty())),
            Err(Error::InvalidTextureUsage { .. })
        ));
        let t = image(&g, 3, 2, &[255, 0, 0, 255].repeat(6));
        assert!(matches!(
            t.write(&[0; 23]),
            Err(Error::InvalidTextureData {
                expected: 24,
                actual: 23
            })
        ));
        for (origin, size) in [([0, 0], [0, 1]), ([2, 0], [2, 1]), ([u32::MAX, 0], [1, 1])] {
            assert!(matches!(
                t.write_region(origin, size, &[0; 4]),
                Err(Error::InvalidTextureRegion)
            ));
        }
        t.write_region([1, 0], [1, 2], &[0, 255, 0, 255].repeat(2))
            .unwrap();
        let mut r = TextureRenderer::new(&g);
        let b = r
            .create_binding(
                t.view(),
                TextureBindingOptions::new().filter(TextureFilter::Nearest),
            )
            .unwrap();
        let mut target = output(&g, 1);
        let bytes = pixels(&g, &mut target, |f| {
            r.draw(
                &mut f.render_pass().begin().unwrap(),
                &b,
                TextureDraw::default(),
            )
            .unwrap();
        });
        assert_eq!(pixel(&bytes, 8, 8), [255, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 32, 8), [0, 255, 0, 255]);
        assert_eq!(pixel(&bytes, 56, 56), [255, 0, 0, 255]);
        assert!(scope.pop().await.is_none());
    });
}
#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn changing_draws_reuse_binding_and_preserve_pixels_and_parameters() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let scope = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let t = image(
            &g,
            2,
            2,
            &[
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ],
        );
        let mut r = TextureRenderer::new(&g);
        let mut target = output(&g, 1);
        let b = r
            .create_binding(
                t.view(),
                TextureBindingOptions::new().filter(TextureFilter::Nearest),
            )
            .unwrap();
        let group = b.group.clone();
        r.prepare(&b, &target.render_format()).unwrap();
        let pipelines = r.pipelines.len();
        for _ in 0..2 {
            let bytes = pixels(&g, &mut target, |f| {
                let mut pass = f.render_pass().begin().unwrap();
                r.draw(
                    &mut pass,
                    &b,
                    TextureDraw::new(Rect::new(0., 0., 32., 64.)).uv(UvRect::new(0., 0., 0.5, 0.5)),
                )
                .unwrap();
                r.draw(
                    &mut pass,
                    &b,
                    TextureDraw::normalized(Rect::new(0.5, 0., 0.5, 1.))
                        .uv(UvRect::new(0.5, 0., 0.5, 0.5))
                        .tint([1., 1., 1., 0.5]),
                )
                .unwrap();
                assert!(matches!(
                    r.draw(&mut pass, &b, TextureDraw::new(Rect::new(0., 0., -1., 1.))),
                    Err(Error::InvalidTextureDraw)
                ));
                assert!(matches!(
                    r.draw(&mut pass, &b, TextureDraw::default().tint([1., 1., 1., 2.])),
                    Err(Error::InvalidTextureDraw)
                ));
            });
            assert_eq!(pixel(&bytes, 8, 8), [255, 0, 0, 255]);
            near(pixel(&bytes, 56, 8), [0, 128, 0, 128]);
            assert_eq!(group, b.group);
            assert_eq!(pipelines, r.pipelines.len());
            assert_eq!(g.upload_pool.lock().unwrap().len(), 1);
        }
        let bytes = pixels(&g, &mut target, |f| {
            let mut pass = f.render_pass().begin().unwrap();
            r.draw_many(
                &mut pass,
                &b,
                &[
                    TextureDraw::new(Rect::new(0., 0., 16., 16.))
                        .uv(UvRect::new(0., 0., 0.5, 0.5))
                        .transform([1., 0., 0., 1., 16., 16.]),
                    TextureDraw::new(Rect::new(32., 32., 16., 16.))
                        .uv(UvRect::new(0.5, 0., 0.5, 0.5)),
                ],
            )
            .unwrap();
        });
        assert_eq!(pixel(&bytes, 20, 20), [255, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 40, 40), [0, 255, 0, 255]);
        assert_eq!(pixel(&bytes, 4, 4), [0; 4]);
        // Nonidentical instance data across chunk/page boundaries preserves draw order.
        let mut many = vec![TextureDraw::default().tint([1., 1., 1., 0.]); 1100];
        many[1099] = TextureDraw::default().uv(UvRect::new(0., 0., 0.5, 0.5));
        let bytes = pixels(&g, &mut target, |f| {
            r.draw_many(&mut f.render_pass().begin().unwrap(), &b, &many)
                .unwrap()
        });
        assert_eq!(pixel(&bytes, 32, 32), [255, 0, 0, 255]);
        assert!(scope.pop().await.is_none());
    });
}
#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn filtering_srgb_alpha_and_material_blending() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let scope = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut r = TextureRenderer::new(&g);
        let mut target = output(&g, 1);
        let t = image(
            &g,
            2,
            2,
            &[
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ],
        );
        let b = r
            .create_binding(t.view(), TextureBindingOptions::new())
            .unwrap();
        let bytes = pixels(&g, &mut target, |f| {
            r.draw(
                &mut f.render_pass().begin().unwrap(),
                &b,
                TextureDraw::default().uv(UvRect::new(0.5, 0.5, 0., 0.)),
            )
            .unwrap()
        });
        near(pixel(&bytes, 32, 32), [128, 128, 128, 255]);
        let srgb = g.create_texture(TextureOptions::new(1, 1)).unwrap();
        srgb.write(&[128, 128, 128, 255]).unwrap();
        let b = r
            .create_binding(srgb.view(), TextureBindingOptions::new())
            .unwrap();
        let bytes = pixels(&g, &mut target, |f| {
            r.draw(
                &mut f.render_pass().begin().unwrap(),
                &b,
                TextureDraw::default(),
            )
            .unwrap()
        });
        near(pixel(&bytes, 32, 32), [55, 55, 55, 255]);
        for (alpha, bytes) in [
            (TextureAlpha::Straight, [255, 64, 0, 128]),
            (TextureAlpha::Premultiplied, [128, 32, 0, 128]),
        ] {
            let t = image(&g, 1, 1, &bytes);
            let b = r
                .create_binding(t.view(), TextureBindingOptions::new().alpha(alpha))
                .unwrap();
            for blend in [TextureBlend::Alpha, TextureBlend::Replace] {
                let m = g.create_texture_material(TextureMaterialOptions::new().blend(blend));
                r.try_prepare_material(&b, &m, &target.render_format())
                    .await
                    .unwrap();
                let bytes = pixels(&g, &mut target, |f| {
                    r.draw_with_material(
                        &mut f
                            .render_pass()
                            .clear_color(wgpu::Color::BLACK)
                            .begin()
                            .unwrap(),
                        &b,
                        &m,
                        TextureDraw::default().tint([0.5, 1., 1., 0.5]),
                    )
                    .unwrap()
                });
                near(
                    pixel(&bytes, 32, 32),
                    [
                        32,
                        16,
                        0,
                        if blend == TextureBlend::Alpha {
                            255
                        } else {
                            64
                        },
                    ],
                );
            }
        }
        assert!(scope.pop().await.is_none());
    });
}
#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn live_sources_follow_resize_and_snapshot_bindings_retain_recorded_views() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let scope = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut r = TextureRenderer::new(&g);
        let mut other = TextureRenderer::new(&g);
        let mut source = output(&g, 4);
        let mut target = output(&g, 1);
        let live = source.sampled_color();
        let b = r.create_sampled_binding(&live).unwrap();
        let old = live.view().unwrap();
        for count in [1, 4] {
            source.resize(32, 32).unwrap();
            source.set_sample_count(count).unwrap();
            let bytes = pixels(&g, &mut target, |f| {
                drop(
                    f.render_to(&mut source)
                        .clear_color(wgpu::Color::GREEN)
                        .begin()
                        .unwrap(),
                );
                other
                    .draw(
                        &mut f.render_pass().begin().unwrap(),
                        &b,
                        TextureDraw::default(),
                    )
                    .unwrap();
            });
            assert_eq!(pixel(&bytes, 32, 32), [0, 255, 0, 255]);
            assert_ne!(old, live.view().unwrap());
        }
        source.resize(0, 0).unwrap();
        assert!(matches!(live.view(), Err(Error::TargetSuspended)));
        let red = image(&g, 1, 1, &[255, 0, 0, 255]);
        let green = image(&g, 1, 1, &[0, 255, 0, 255]);
        let mut b = r
            .create_binding(red.view(), TextureBindingOptions::new())
            .unwrap();
        let group = b.group.clone();
        r.rebind(&mut b, red.view()).unwrap();
        assert_eq!(group, b.group);
        let bytes = pixels(&g, &mut target, |f| {
            let mut p = f.render_pass().scissor_rect(0, 0, 32, 64).begin().unwrap();
            r.draw(&mut p, &b, TextureDraw::default()).unwrap();
            r.rebind(&mut b, green.view()).unwrap();
            p.set_scissor_rect(32, 0, 32, 64).unwrap();
            other.draw(&mut p, &b, TextureDraw::default()).unwrap();
        });
        assert_eq!(pixel(&bytes, 8, 8), [255, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 56, 8), [0, 255, 0, 255]);
        assert!(scope.pop().await.is_none());
    });
}
#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn independent_recordings_lease_upload_pages_until_submission() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let scope = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let t = image(&g, 1, 1, &[255; 4]);
        let mut r = TextureRenderer::new(&g);
        let b = r
            .create_binding(t.view(), TextureBindingOptions::new())
            .unwrap();
        let mut a = output(&g, 1);
        let mut second = output(&g, 1);
        let mut first = a.begin_frame().unwrap();
        r.draw(
            &mut first.render_pass().begin().unwrap(),
            &b,
            TextureDraw::default().tint([1., 0., 0., 1.]),
        )
        .unwrap();
        let green = pixels(&g, &mut second, |f| {
            r.draw(
                &mut f.render_pass().begin().unwrap(),
                &b,
                TextureDraw::default().tint([0., 1., 0., 1.]),
            )
            .unwrap()
        });
        assert_eq!(pixel(&green, 32, 32), [0, 255, 0, 255]);
        first.finish().unwrap();
        let red = pixels(&g, &mut a, |f| {
            drop(f.render_pass().load_all().begin().unwrap())
        });
        assert_eq!(pixel(&red, 32, 32), [255, 0, 0, 255]);
        // Abandoned draws upload nothing and return their page for subsequent recording.
        {
            let mut f = a.begin_frame().unwrap();
            r.draw(
                &mut f.render_pass().begin().unwrap(),
                &b,
                TextureDraw::default().tint([0., 0., 1., 1.]),
            )
            .unwrap();
        }
        let red = pixels(&g, &mut a, |f| {
            drop(f.render_pass().load_all().begin().unwrap())
        });
        assert_eq!(pixel(&red, 32, 32), [255, 0, 0, 255]);
        assert!(scope.pop().await.is_none());
    });
}
#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn texture_stencil_clipping_and_raw_state_restoration() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let scope = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let t = image(&g, 1, 1, &[255, 0, 0, 255]);
        let mut r = TextureRenderer::new(&g);
        let b = r
            .create_binding(t.view(), TextureBindingOptions::new())
            .unwrap();
        let mut meshes = MeshRenderer::new(&g);
        let mask = g
            .create_mesh(
                &[
                    Vertex::new([-1., 1., 0.], [1.; 4]),
                    Vertex::new([-1., -1., 0.], [1.; 4]),
                    Vertex::new([0., 1., 0.], [1.; 4]),
                    Vertex::new([0., -1., 0.], [1.; 4]),
                ],
                &[0, 1, 2, 2, 1, 3],
            )
            .unwrap();
        let stencil = |compare, pass_op, write_mask| wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth24PlusStencil8,
            depth_write_enabled: Some(false),
            depth_compare: None,
            stencil: wgpu::StencilState {
                front: wgpu::StencilFaceState {
                    compare,
                    pass_op,
                    ..Default::default()
                },
                back: wgpu::StencilFaceState {
                    compare,
                    pass_op,
                    ..Default::default()
                },
                read_mask: 255,
                write_mask,
            },
            bias: Default::default(),
        };
        let mut mask_options = crate::MaterialOptions::new(meshes.default_material().shader())
            .depth_stencil(Some(stencil(
                wgpu::CompareFunction::Always,
                wgpu::StencilOperation::Replace,
                255,
            )));
        mask_options.write_mask = wgpu::ColorWrites::empty();
        let mask_material = g.create_material(mask_options);
        let material =
            g.create_texture_material(TextureMaterialOptions::new().depth_stencil(Some(stencil(
                wgpu::CompareFunction::Equal,
                wgpu::StencilOperation::Keep,
                0,
            ))));
        let mut target = g
            .create_framebuffer(
                FramebufferOptions::new(64, 64)
                    .sample_count(4)
                    .depth_stencil(wgpu::TextureFormat::Depth24PlusStencil8)
                    .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC),
            )
            .unwrap();
        let bytes = pixels(&g, &mut target, |f| {
            {
                let mut p = f.render_pass().stencil_reference(1).begin().unwrap();
                meshes
                    .draw_with_material(&mut p, &mask, &mask_material)
                    .unwrap();
            }
            let mut p = f
                .render_pass()
                .load_all()
                .stencil_reference(1)
                .begin()
                .unwrap();
            p.as_wgpu().set_stencil_reference(0);
            r.draw_with_material(&mut p, &b, &material, TextureDraw::default())
                .unwrap();
        });
        assert_eq!(pixel(&bytes, 8, 32), [255, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 56, 32), [0; 4]);
        assert!(scope.pop().await.is_none());
    });
}
#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn invalid_sources_feedback_and_devices_are_rejected() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let scope = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut r = TextureRenderer::new(&g);
        let integer = g
            .create_texture(TextureOptions::new(1, 1).format(wgpu::TextureFormat::Rgba8Uint))
            .unwrap();
        assert!(matches!(
            r.create_binding(integer.view(), TextureBindingOptions::new()),
            Err(Error::InvalidTextureSource { .. })
        ));
        let float = g
            .create_texture(TextureOptions::new(1, 1).format(wgpu::TextureFormat::R32Float))
            .unwrap();
        float.write(bytemuck::bytes_of(&0.5f32)).unwrap();
        assert!(matches!(
            r.create_binding(float.view(), TextureBindingOptions::new()),
            Err(Error::InvalidTextureSource { .. })
        ));
        let b = r
            .create_binding(
                float.view(),
                TextureBindingOptions::new().filter(TextureFilter::Nearest),
            )
            .unwrap();
        let mut target = output(&g, 1);
        let bytes = pixels(&g, &mut target, |f| {
            r.draw(
                &mut f.render_pass().begin().unwrap(),
                &b,
                TextureDraw::default(),
            )
            .unwrap()
        });
        near(pixel(&bytes, 32, 32), [128, 0, 0, 255]);
        let feedback = r
            .create_binding(target.color_view().unwrap(), TextureBindingOptions::new())
            .unwrap();
        let foreign = GraphicsContext::headless().await.unwrap();
        let ft = image(&foreign, 1, 1, &[255; 4]);
        let fr = TextureRenderer::new(&foreign);
        let fb = fr
            .create_binding(ft.view(), TextureBindingOptions::new())
            .unwrap();
        let mut f = target.begin_frame().unwrap();
        let mut p = f.render_pass().begin().unwrap();
        assert!(matches!(
            r.draw(&mut p, &feedback, TextureDraw::default()),
            Err(Error::TextureFeedback)
        ));
        assert!(matches!(
            r.draw(&mut p, &fb, TextureDraw::default()),
            Err(Error::DeviceMismatch)
        ));
        drop(p);
        drop(f);
        assert!(scope.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn prepared_draws_reuse_static_instance_storage_and_validate_pixel_viewports() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let scope = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let texture = image(&g, 1, 1, &[255; 4]);
        let mut renderer = TextureRenderer::new(&g);
        let binding = renderer
            .create_binding(texture.view(), TextureBindingOptions::new())
            .unwrap();
        let prepared = renderer
            .prepare_draws(
                &[
                    TextureDraw::new(Rect::new(0., 0., 32., 64.)).tint([1., 0., 0., 1.]),
                    TextureDraw::new(Rect::new(32., 0., 32., 64.)).tint([0., 1., 0., 1.]),
                ],
                [64., 64.],
            )
            .unwrap();
        let mut target = output(&g, 1);
        for _ in 0..2 {
            let bytes = pixels(&g, &mut target, |frame| {
                renderer
                    .draw_prepared(
                        &mut frame.render_pass().begin().unwrap(),
                        &binding,
                        &prepared,
                    )
                    .unwrap();
            });
            assert_eq!(pixel(&bytes, 8, 32), [255, 0, 0, 255]);
            assert_eq!(pixel(&bytes, 56, 32), [0, 255, 0, 255]);
            assert!(g.upload_pool.lock().unwrap().is_empty());
        }
        let normalized = renderer
            .prepare_draws(&[TextureDraw::default()], [1., 1.])
            .unwrap();
        let bytes = pixels(&g, &mut target, |frame| {
            let mut pass = frame
                .render_pass()
                .viewport(0., 0., 32., 32., 0., 1.)
                .begin()
                .unwrap();
            assert!(matches!(
                renderer.draw_prepared(&mut pass, &binding, &prepared),
                Err(Error::InvalidTextureDraw)
            ));
            renderer
                .draw_prepared(&mut pass, &binding, &normalized)
                .unwrap();
        });
        assert_eq!(pixel(&bytes, 8, 8), [255; 4]);
        assert_eq!(pixel(&bytes, 56, 56), [0; 4]);
        assert!(scope.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn fixed_workload_reuses_upload_pages_and_prepared_pipeline_after_warmup() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let scope = graphics
            .device()
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let mut target = graphics
            .create_framebuffer(FramebufferOptions::new(64, 64))
            .unwrap();
        let image = graphics.create_texture(TextureOptions::new(1, 1)).unwrap();
        image.write(&[255; 4]).unwrap();
        let mut renderer = TextureRenderer::new(&graphics);
        let binding = renderer
            .create_binding(image.view(), TextureBindingOptions::new())
            .unwrap();
        renderer.prepare(&binding, &target.render_format()).unwrap();
        let pipeline = renderer.pipelines.values().next().unwrap().clone();
        let draws = vec![TextureDraw::default(); 2050]; // Three 64 KiB pages.
        {
            let mut frame = target.begin_frame().unwrap();
            {
                let mut pass = frame.render_pass().begin().unwrap();
                renderer.draw_many(&mut pass, &binding, &draws).unwrap();
            }
            frame.finish().unwrap();
        }
        let pages: Vec<_> = graphics
            .upload_pool
            .lock()
            .unwrap()
            .iter()
            .map(|(buffer, bytes)| (buffer.clone(), bytes.capacity()))
            .collect();
        assert_eq!(pages.len(), 3);
        let scratch_capacity = renderer.parameters.capacity();
        for iteration in 0..16 {
            if iteration % 4 == 0 {
                let size = if iteration % 8 == 0 { 32 } else { 64 };
                target.resize(size, size).unwrap();
            }
            let mut frame = target.begin_frame().unwrap();
            {
                let mut pass = frame.render_pass().begin().unwrap();
                renderer.draw_many(&mut pass, &binding, &draws).unwrap();
            }
            if iteration % 2 == 0 {
                frame.finish().unwrap();
            } else {
                drop(frame);
            } // Abandonment also returns leased pages.
            let pool = graphics.upload_pool.lock().unwrap();
            assert_eq!(pool.len(), pages.len());
            for (buffer, bytes) in pool.iter() {
                let original = pages.iter().find(|(old, _)| old == buffer).unwrap();
                assert_eq!(bytes.capacity(), original.1);
                assert_eq!(buffer.size(), 64 * 1024);
            }
            assert_eq!(renderer.parameters.capacity(), scratch_capacity);
            assert_eq!(renderer.pipelines.len(), 1);
            assert_eq!(renderer.pipelines.values().next().unwrap(), &pipeline);
        }
        graphics
            .device()
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        assert!(scope.pop().await.is_none());
    });
}
#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn straight_alpha_filters_premultiplied_texels() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let scope = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut r = TextureRenderer::new(&g);
        let mut target = output(&g, 1);
        // Opaque white beside transparent black: filtering straight RGB would darken
        // the edge, while premultiplied filtering keeps every blend pure white.
        let t = image(&g, 2, 1, &[255, 255, 255, 255, 0, 0, 0, 0]);
        let m =
            g.create_texture_material(TextureMaterialOptions::new().blend(TextureBlend::Replace));
        for filter in [TextureFilter::Linear, TextureFilter::Nearest] {
            let b = r
                .create_binding(t.view(), TextureBindingOptions::new().filter(filter))
                .unwrap();
            r.try_prepare_material(&b, &m, &target.render_format())
                .await
                .unwrap();
            let bytes = pixels(&g, &mut target, |f| {
                r.draw_with_material(
                    &mut f.render_pass().begin().unwrap(),
                    &b,
                    &m,
                    TextureDraw::default(),
                )
                .unwrap()
            });
            for x in 0..64 {
                let [red, green, blue, alpha] = pixel(&bytes, x, 32);
                for channel in [red, green, blue] {
                    assert!(
                        channel.abs_diff(alpha) <= 1,
                        "{filter:?} x={x}: {red} vs {alpha}"
                    );
                }
            }
            if filter == TextureFilter::Linear {
                // Pixel 32 samples texel coordinate 0.516: 48.4% of the opaque texel.
                near(pixel(&bytes, 32, 32), [124, 124, 124, 124]);
            }
        }
        assert!(scope.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn straight_alpha_with_uniform_footprint_alpha_matches_premultiplied_filtering() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let scope = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut r = TextureRenderer::new(&g);
        let mut target = output(&g, 1);
        let m =
            g.create_texture_material(TextureMaterialOptions::new().blend(TextureBlend::Replace));
        // Equal alpha across the footprint takes the single-sample path; it must
        // match filtering the same texels stored premultiplied.
        for (straight, premultiplied) in [
            (
                [255, 0, 0, 255, 0, 0, 255, 255],
                [255, 0, 0, 255, 0, 0, 255, 255],
            ),
            (
                [255, 0, 0, 128, 0, 0, 255, 128],
                [128, 0, 0, 128, 0, 0, 128, 128],
            ),
        ] {
            let mut rows = Vec::new();
            for (bytes, alpha) in [
                (straight, TextureAlpha::Straight),
                (premultiplied, TextureAlpha::Premultiplied),
            ] {
                let t = image(&g, 2, 1, &bytes);
                let b = r
                    .create_binding(t.view(), TextureBindingOptions::new().alpha(alpha))
                    .unwrap();
                r.try_prepare_material(&b, &m, &target.render_format())
                    .await
                    .unwrap();
                let bytes = pixels(&g, &mut target, |f| {
                    r.draw_with_material(
                        &mut f.render_pass().begin().unwrap(),
                        &b,
                        &m,
                        TextureDraw::default(),
                    )
                    .unwrap()
                });
                rows.push((0..64).map(|x| pixel(&bytes, x, 32)).collect::<Vec<_>>());
            }
            for (a, b) in rows[0].iter().zip(&rows[1]) {
                near(*a, *b);
            }
            // The middle blends both texels rather than snapping to one.
            assert!(
                rows[0][32][0] > 40 && rows[0][32][2] > 40,
                "{:?}",
                rows[0][32]
            );
        }
        assert!(scope.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn material_pipeline_cache_churn_retains_clones_and_reuses_live_variants() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let mut renderer = TextureRenderer::new(&g);
        let target = output(&g, 1);
        let texture = image(&g, 1, 1, &[255; 4]);
        let binding = renderer
            .create_binding(texture.view(), TextureBindingOptions::new())
            .unwrap();
        let material = g.create_texture_material(TextureMaterialOptions::new());
        let live = material.clone();
        renderer
            .prepare_material(&binding, &material, &target.render_format())
            .unwrap();
        renderer
            .try_prepare_material(&binding, &material, &target.render_format())
            .await
            .unwrap();
        drop(material);
        let pipeline = renderer.pipelines.values().next().unwrap().clone();
        for _ in 0..32 {
            for _ in 0..4 {
                let transient = g.create_texture_material(TextureMaterialOptions::new());
                renderer
                    .prepare_material(&binding, &transient, &target.render_format())
                    .unwrap();
                renderer
                    .prepare_material(
                        &binding,
                        &transient,
                        &crate::RenderFormat::color(wgpu::TextureFormat::Rgba8UnormSrgb, 1),
                    )
                    .unwrap();
            }
            assert_eq!(
                renderer.pipeline_cache_stats(),
                crate::PipelineCacheStats {
                    entries: 9,
                    live_entries: 1,
                    stale_entries: 8,
                }
            );
            assert_eq!(renderer.prune_pipeline_cache(), 8);
            renderer
                .prepare_material(&binding, &live, &target.render_format())
                .unwrap();
            renderer
                .try_prepare_material(&binding, &live, &target.render_format())
                .await
                .unwrap();
            assert_eq!(renderer.pipelines.values().next().unwrap(), &pipeline);
        }
        drop(live);
        assert_eq!(renderer.prune_pipeline_cache(), 1);
        assert_eq!(
            renderer.pipeline_cache_stats(),
            crate::PipelineCacheStats::default()
        );
        assert_eq!(renderer.pipelines.capacity(), 0);
        renderer.prepare(&binding, &target.render_format()).unwrap();
        assert_eq!(renderer.prune_pipeline_cache(), 0);
        assert_eq!(renderer.clear_pipeline_cache(), 1);
        assert_eq!(renderer.clear_pipeline_cache(), 0);
        renderer.prepare(&binding, &target.render_format()).unwrap();
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn pipeline_cache_eviction_preserves_prepared_texture_scope_and_deferred_recording() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let validation = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut renderer = TextureRenderer::new(&g);
        let texture = image(&g, 1, 1, &[0, 255, 0, 255]);
        let binding = renderer
            .create_binding(texture.view(), TextureBindingOptions::new())
            .unwrap();
        let draws = renderer
            .prepare_draws(&[TextureDraw::default()], [64., 64.])
            .unwrap();
        let mut target = output(&g, 1);
        let format = target.render_format();
        let mut recording = target.begin_frame().unwrap();
        {
            let mut pass = recording.render_pass().begin().unwrap();
            let material = g.create_texture_material(TextureMaterialOptions::new());
            let mut scope = renderer
                .bind_prepared_with_material(&mut pass, &binding, &material, &draws)
                .unwrap();
            scope.pass().set_scissor_rect(0, 0, 32, 64).unwrap();
            scope.draw().unwrap();
            drop(material);
            assert_eq!(renderer.prune_pipeline_cache(), 1);
            renderer.prepare(&binding, &format).unwrap();
            assert_eq!(renderer.clear_pipeline_cache(), 1);
            scope.pass().set_scissor_rect(32, 0, 32, 64).unwrap();
            scope.draw().unwrap();
        }
        drop(renderer); // Deferred recording must own everything needed for submission.
        recording.finish().unwrap();
        let bytes = pixels(&g, &mut target, |frame| {
            drop(frame.render_pass().load_all().begin().unwrap());
        });
        for x in [16, 48] {
            near(pixel(&bytes, x, 32), [0, 255, 0, 255]);
        }
        assert!(validation.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn checked_preparation_tracks_validation_provenance_and_eviction() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let mut renderer = TextureRenderer::new(&g);
        let texture = image(&g, 1, 1, &[255; 4]);
        let binding = renderer
            .create_binding(texture.view(), TextureBindingOptions::new())
            .unwrap();
        let material = g.create_texture_material(TextureMaterialOptions::new());
        let shader = g.device().create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(
                "@vertex fn vs() -> @builtin(position) vec4<f32> { return vec4<f32>(0.); } @fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(1.); }".into(),
            ),
        });
        let mut options = TextureMaterialOptions::new();
        options.shader = Some(&shader);
        options.vertex_entry = "missing_vertex";
        options.fragment_entry = "fs";
        let invalid = g.create_texture_material(options);
        let format = crate::RenderFormat::color(wgpu::TextureFormat::Rgba8Unorm, 1);
        let validation = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        renderer
            .prepare_material(&binding, &material, &format)
            .unwrap();
        assert!(validation.pop().await.is_none());
        let unchecked = renderer.pipelines.values().next().unwrap().clone();
        assert!(!unchecked.is_validated());
        renderer
            .try_prepare_material(&binding, &material, &format)
            .await
            .unwrap();
        let verified = renderer.pipelines.values().next().unwrap().clone();
        assert!(verified.is_validated());
        assert_ne!(unchecked, verified); // The unchecked handle was actually revalidated.
        for _ in 0..3 {
            renderer
                .prepare_material(&binding, &material, &format)
                .unwrap();
            renderer
                .try_prepare_material(&binding, &material, &format)
                .await
                .unwrap();
            assert_eq!(renderer.pipelines.values().next().unwrap(), &verified);
        }

        // Synchronous preparation still returns Ok and reports through the caller's scope.
        let validation = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        renderer
            .prepare_material(&binding, &invalid, &format)
            .unwrap();
        assert!(validation.pop().await.is_some());
        assert_eq!(renderer.pipeline_cache_stats().entries, 2);
        for _ in 0..2 {
            assert!(matches!(
                renderer
                    .try_prepare_material(&binding, &invalid, &format)
                    .await,
                Err(Error::Validation(_))
            ));
            assert_eq!(renderer.pipeline_cache_stats().entries, 1);
            assert_eq!(renderer.pipelines.values().next().unwrap(), &verified);
        }
        assert_eq!(renderer.prune_pipeline_cache(), 0);
        assert_eq!(renderer.clear_pipeline_cache(), 1);
        renderer
            .prepare_material(&binding, &material, &format)
            .unwrap();
        assert!(!renderer.pipelines.values().next().unwrap().is_validated());
        renderer
            .try_prepare_material(&binding, &material, &format)
            .await
            .unwrap();
        let recreated = renderer.pipelines.values().next().unwrap().clone();
        assert!(recreated.is_validated());
        assert_ne!(verified, recreated);
        // Pruning treats verified and unchecked variants alike and retains no provenance.
        renderer
            .prepare_material(
                &binding,
                &material,
                &crate::RenderFormat::color(wgpu::TextureFormat::Rgba8UnormSrgb, 1),
            )
            .unwrap();
        drop(material);
        assert_eq!(renderer.prune_pipeline_cache(), 2);
        assert_eq!(
            renderer.pipeline_cache_stats(),
            crate::PipelineCacheStats::default()
        );
        assert_eq!(renderer.clear_pipeline_cache(), 0);
    });
}
