use crate::{Framebuffer, FramebufferOptions, RenderTarget, Vertex, framebuffer::tests::pixels};

use super::*;

fn target(graphics: &GraphicsContext, format: wgpu::TextureFormat, count: u32) -> Framebuffer {
    graphics
        .create_framebuffer(
            FramebufferOptions::new(64, 64)
                .sample_count(count)
                .depth_stencil(format)
                .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC),
        )
        .unwrap()
}

fn quad(graphics: &GraphicsContext, center: f32, extent: f32, z: f32, color: [f32; 4]) -> Mesh {
    graphics
        .create_mesh(
            &[
                Vertex::new([center - extent, -extent, z], color),
                Vertex::new([center + extent, -extent, z], color),
                Vertex::new([center + extent, extent, z], color),
                Vertex::new([center - extent, extent, z], color),
            ],
            &[0, 1, 2, 0, 2, 3],
        )
        .unwrap()
}

fn state(format: wgpu::TextureFormat) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format,
        depth_write_enabled: format.has_depth_aspect().then_some(false),
        depth_compare: None,
        stencil: Default::default(),
        bias: Default::default(),
    }
}

fn material(
    graphics: &GraphicsContext,
    renderer: &MeshRenderer,
    state: wgpu::DepthStencilState,
) -> Material {
    graphics.create_material(
        MaterialOptions::new(renderer.default_material().shader())
            .blend(None)
            .depth_stencil(Some(state)),
    )
}

fn at(image: &[u8], x: usize) -> &[u8] {
    &image[32 * 256 + x * 4..32 * 256 + x * 4 + 4]
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn depth_tests_occlude_independent_of_order_and_persist_across_passes() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let near = quad(&graphics, 0.0, 0.4, 0.2, [1.0, 0.0, 0.0, 1.0]);
        let far = quad(&graphics, 0.0, 0.8, 0.8, [0.0, 0.0, 1.0, 1.0]);
        for format in [
            wgpu::TextureFormat::Depth24Plus,
            wgpu::TextureFormat::Depth32Float,
        ] {
            for count in [1, 4] {
                let mut target = target(&graphics, format, count);
                let mut renderer = MeshRenderer::new(&graphics);
                let mut depth_state = state(format);
                depth_state.depth_write_enabled = Some(true);
                depth_state.depth_compare = Some(wgpu::CompareFunction::Less);
                let material = material(&graphics, &renderer, depth_state);
                renderer
                    .prepare_material(&material, &target.render_format())
                    .unwrap();
                let cached = renderer.pipelines.clone();
                let image = pixels(&graphics, &mut target, |frame| {
                    assert_eq!(frame.depth_stencil_format(), Some(format));
                    let mut pass = frame
                        .render_pass()
                        .clear_color(wgpu::Color::BLACK)
                        .begin()
                        .unwrap();
                    assert_eq!(pass.depth_stencil_format(), Some(format));
                    renderer
                        .draw_with_material(&mut pass, &near, &material)
                        .unwrap();
                    renderer
                        .draw_with_material(&mut pass, &far, &material)
                        .unwrap();
                });
                assert_eq!(at(&image, 32), [255, 0, 0, 255]);
                assert_eq!(at(&image, 8), [0, 0, 255, 255]);
                let reversed = pixels(&graphics, &mut target, |frame| {
                    let mut pass = frame
                        .render_pass()
                        .clear_color(wgpu::Color::BLACK)
                        .begin()
                        .unwrap();
                    renderer
                        .draw_with_material(&mut pass, &far, &material)
                        .unwrap();
                    renderer
                        .draw_with_material(&mut pass, &near, &material)
                        .unwrap();
                });
                assert_eq!(image, reversed);
                let sequential = pixels(&graphics, &mut target, |frame| {
                    {
                        let mut pass = frame
                            .render_pass()
                            .clear_color(wgpu::Color::BLACK)
                            .begin()
                            .unwrap();
                        renderer
                            .draw_with_material(&mut pass, &near, &material)
                            .unwrap();
                    }
                    let mut pass = frame
                        .render_pass()
                        .load_color()
                        .load_depth()
                        .begin()
                        .unwrap();
                    renderer
                        .draw_with_material(&mut pass, &far, &material)
                        .unwrap();
                });
                assert_eq!(image, sequential);
                let persisted = pixels(&graphics, &mut target, |frame| {
                    let mut pass = frame
                        .render_pass()
                        .load_color()
                        .load_depth()
                        .begin()
                        .unwrap();
                    renderer
                        .draw_with_material(&mut pass, &far, &material)
                        .unwrap();
                });
                assert_eq!(persisted, image);
                assert_eq!(renderer.pipelines, cached, "prepared depth pipeline reused");
                // Default shading can overlay a depth-enabled scene without testing or writing depth.
                let overlay = pixels(&graphics, &mut target, |frame| {
                    let mut pass = frame
                        .render_pass()
                        .load_color()
                        .load_depth()
                        .begin()
                        .unwrap();
                    renderer.draw(&mut pass, &far).unwrap();
                });
                assert_eq!(at(&overlay, 32), [0, 0, 255, 255]);
                let preserved_depth = pixels(&graphics, &mut target, |frame| {
                    let mut pass = frame
                        .render_pass()
                        .clear_color(wgpu::Color::BLACK)
                        .load_depth()
                        .begin()
                        .unwrap();
                    renderer
                        .draw_with_material(&mut pass, &far, &material)
                        .unwrap();
                });
                assert_eq!(at(&preserved_depth, 32), [0, 0, 0, 255]);
            }
        }
        // Reversed-Z is a pass clear/material comparison choice, without a renderer mode.
        let mut target = target(&graphics, wgpu::TextureFormat::Depth32Float, 1);
        let mut renderer = MeshRenderer::new(&graphics);
        let mut reversed_state = state(wgpu::TextureFormat::Depth32Float);
        reversed_state.depth_write_enabled = Some(true);
        reversed_state.depth_compare = Some(wgpu::CompareFunction::Greater);
        let reversed_material = material(&graphics, &renderer, reversed_state);
        let red = quad(&graphics, 0.0, 0.4, 0.8, [1.0, 0.0, 0.0, 1.0]);
        let blue = quad(&graphics, 0.0, 0.8, 0.2, [0.0, 0.0, 1.0, 1.0]);
        let image = pixels(&graphics, &mut target, |frame| {
            let mut pass = frame.render_pass().clear_depth(0.0).begin().unwrap();
            renderer
                .draw_with_material(&mut pass, &red, &reversed_material)
                .unwrap();
            renderer
                .draw_with_material(&mut pass, &blue, &reversed_material)
                .unwrap();
        });
        assert_eq!(at(&image, 32), [255, 0, 0, 255]);
        assert_eq!(at(&image, 8), [0, 0, 255, 255]);
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn stencil_masks_dynamic_references_and_read_only_passes_clip_meshes() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let center_mask = quad(&graphics, 0.0, 0.25, 0.0, [1.0; 4]);
        let left_mask = quad(&graphics, -0.55, 0.2, 0.0, [1.0; 4]);
        let green = quad(&graphics, 0.0, 0.9, 0.0, [0.0, 1.0, 0.0, 1.0]);
        let blue = quad(&graphics, 0.0, 0.9, 0.0, [0.0, 0.0, 1.0, 1.0]);
        for format in [
            wgpu::TextureFormat::Stencil8,
            wgpu::TextureFormat::Depth24PlusStencil8,
        ] {
            for count in [1, 4] {
                let mut target = target(&graphics, format, count);
                let mut renderer = MeshRenderer::new(&graphics);
                let mut write_state = state(format);
                let face = wgpu::StencilFaceState {
                    compare: wgpu::CompareFunction::Always,
                    pass_op: wgpu::StencilOperation::Replace,
                    ..Default::default()
                };
                write_state.stencil = wgpu::StencilState {
                    front: face,
                    back: face,
                    read_mask: 0xff,
                    write_mask: 0xff,
                };
                let writer = graphics.create_material(
                    MaterialOptions::new(renderer.default_material().shader())
                        .write_mask(wgpu::ColorWrites::empty())
                        .depth_stencil(Some(write_state)),
                );
                let mut test_state = state(format);
                let face = wgpu::StencilFaceState {
                    compare: wgpu::CompareFunction::Equal,
                    ..Default::default()
                };
                test_state.stencil = wgpu::StencilState {
                    front: face,
                    back: face,
                    read_mask: 0xff,
                    write_mask: 0,
                };
                let reader = material(&graphics, &renderer, test_state);
                let image = pixels(&graphics, &mut target, |frame| {
                    {
                        let mut pass = frame
                            .render_pass()
                            .clear_color(wgpu::Color::BLACK)
                            .stencil_reference(7)
                            .begin()
                            .unwrap();
                        renderer
                            .draw_with_material(&mut pass, &center_mask, &writer)
                            .unwrap();
                        pass.set_stencil_reference(3);
                        renderer
                            .draw_with_material(&mut pass, &left_mask, &writer)
                            .unwrap();
                    }
                    let mut builder = frame
                        .render_pass()
                        .load_color()
                        .stencil_ops(None)
                        .stencil_reference(7);
                    if format.has_depth_aspect() {
                        builder = builder.depth_ops(None);
                    }
                    let mut pass = builder.begin().unwrap();
                    assert!(pass.stencil_read_only());
                    assert_eq!(pass.depth_read_only(), format.has_depth_aspect());
                    assert!(matches!(
                        renderer.draw_with_material(&mut pass, &center_mask, &writer),
                        Err(Error::ReadOnlyStencil)
                    ));
                    // Raw reference changes do not replace the value selected through the wrapper.
                    pass.as_wgpu().set_stencil_reference(99);
                    renderer
                        .draw_with_material(&mut pass, &green, &reader)
                        .unwrap();
                    pass.set_stencil_reference(3);
                    renderer
                        .draw_with_material(&mut pass, &blue, &reader)
                        .unwrap();
                });
                assert_eq!(at(&image, 32), [0, 255, 0, 255]);
                assert_eq!(at(&image, 16), [0, 0, 255, 255]);
                assert_eq!(at(&image, 48), [0, 0, 0, 255]);
                let cached = renderer.pipelines.clone();
                let persisted = pixels(&graphics, &mut target, |frame| {
                    let mut pass = frame
                        .render_pass()
                        .load_color()
                        .load_stencil()
                        .stencil_reference(7)
                        .begin()
                        .unwrap();
                    renderer
                        .draw_with_material(&mut pass, &green, &reader)
                        .unwrap();
                });
                assert_eq!(persisted, image);
                assert_eq!(
                    renderer.pipelines, cached,
                    "reference/load changes do not create pipelines"
                );
            }
        }
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn depth_stencil_validation_discard_and_abandonment_preserve_initialization() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let format = wgpu::TextureFormat::Depth24PlusStencil8;
        let mut target = target(&graphics, format, 1);
        {
            let mut frame = target.begin_frame().unwrap();
            assert!(matches!(
                frame.render_pass().load_depth().begin(),
                Err(Error::UninitializedDepth)
            ));
            assert!(matches!(
                frame.render_pass().load_stencil().begin(),
                Err(Error::UninitializedStencil)
            ));
            for value in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
                assert!(matches!(
                    frame.render_pass().clear_depth(value).begin(),
                    Err(Error::InvalidClearDepth)
                ));
            }
            assert!(matches!(
                frame.render_pass().load_depth().begin(),
                Err(Error::UninitializedDepth)
            ));
            drop(frame.render_pass().begin().unwrap());
            drop(
                frame
                    .render_pass()
                    .load_depth()
                    .load_stencil()
                    .begin()
                    .unwrap(),
            );
        } // Abandoned clears never initialize the resource.
        let mut frame = target.begin_frame().unwrap();
        assert!(matches!(
            frame.render_pass().load_depth().begin(),
            Err(Error::UninitializedDepth)
        ));
        drop(frame.render_pass().begin().unwrap());
        frame.finish().unwrap();

        // An abandoned discard must preserve the previously submitted stored aspects.
        let mut frame = target.begin_frame().unwrap();
        drop(
            frame
                .render_pass()
                .depth_ops(Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Discard,
                }))
                .begin()
                .unwrap(),
        );
        assert!(matches!(
            frame.render_pass().load_depth().begin(),
            Err(Error::UninitializedDepth)
        ));
        drop(frame);
        let mut frame = target.begin_frame().unwrap();
        drop(
            frame
                .render_pass()
                .load_depth()
                .load_stencil()
                .begin()
                .unwrap(),
        );
        drop(
            frame
                .render_pass()
                .depth_ops(Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Discard,
                }))
                .load_stencil()
                .begin()
                .unwrap(),
        );
        frame.finish().unwrap();
        let mut frame = target.begin_frame().unwrap();
        assert!(matches!(
            frame.render_pass().load_depth().begin(),
            Err(Error::UninitializedDepth)
        ));
        drop(
            frame
                .render_pass()
                .clear_depth(1.0)
                .load_stencil()
                .begin()
                .unwrap(),
        );
        drop(
            frame
                .render_pass()
                .load_depth()
                .stencil_ops(Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Discard,
                }))
                .begin()
                .unwrap(),
        );
        assert!(matches!(
            frame.render_pass().load_stencil().begin(),
            Err(Error::UninitializedStencil)
        ));
        drop(
            frame
                .render_pass()
                .load_depth()
                .clear_stencil(0)
                .begin()
                .unwrap(),
        );
        drop(
            frame
                .render_pass()
                .depth_ops(None)
                .stencil_ops(None)
                .begin()
                .unwrap(),
        );
        frame.finish().unwrap();
        let mut frame = target.begin_frame().unwrap();
        drop(
            frame
                .render_pass()
                .load_depth()
                .load_stencil()
                .begin()
                .unwrap(),
        );
        drop(frame);

        let mut renderer = MeshRenderer::new(&graphics);
        let mut write_state = state(format);
        write_state.depth_write_enabled = Some(true);
        write_state.depth_compare = Some(wgpu::CompareFunction::Less);
        let writer = material(&graphics, &renderer, write_state);
        let wrong = material(
            &graphics,
            &renderer,
            state(wgpu::TextureFormat::Depth32Float),
        );
        let mesh = quad(&graphics, 0.0, 0.5, 0.5, [1.0; 4]);
        let mut frame = target.begin_frame().unwrap();
        let mut pass = frame.render_pass().depth_ops(None).begin().unwrap();
        assert!(matches!(
            renderer.draw_with_material(&mut pass, &mesh, &writer),
            Err(Error::ReadOnlyDepth)
        ));
        assert!(matches!(
            renderer.draw_with_material(&mut pass, &mesh, &wrong),
            Err(Error::DepthStencilMismatch { .. })
        ));
        renderer.draw(&mut pass, &mesh).unwrap();
        drop(pass);
        frame.finish().unwrap();
        let mut plain = graphics
            .create_framebuffer(FramebufferOptions::new(64, 64))
            .unwrap();
        let mut frame = plain.begin_frame().unwrap();
        assert!(matches!(
            frame.render_pass().clear_depth(1.0).begin(),
            Err(Error::MissingDepthAttachment)
        ));
        assert!(matches!(
            frame.render_pass().clear_stencil(0).begin(),
            Err(Error::MissingStencilAttachment)
        ));
        let mut pass = frame.render_pass().begin().unwrap();
        assert!(matches!(
            renderer.draw_with_material(&mut pass, &mesh, &writer),
            Err(Error::DepthStencilMismatch { .. })
        ));
        renderer.draw(&mut pass, &mesh).unwrap();
        drop(pass);
        frame.finish().unwrap();
        let mut depth_only = target_for_depth_only(&graphics);
        let mut frame = depth_only.begin_frame().unwrap();
        assert!(matches!(
            frame.render_pass().load_stencil().begin(),
            Err(Error::MissingStencilAttachment)
        ));
        drop(frame.render_pass().begin().unwrap());
        frame.finish().unwrap();
    });
}

fn target_for_depth_only(graphics: &GraphicsContext) -> Framebuffer {
    target(graphics, wgpu::TextureFormat::Depth24Plus, 1)
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn depth_stencil_targets_validate_capabilities_and_replace_attachment_generations() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        for format in [
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureFormat::Depth32FloatStencil8,
        ] {
            assert!(matches!(
                graphics.create_framebuffer(FramebufferOptions::new(0, 0).depth_stencil(format)),
                Err(Error::UnsupportedDepthStencilFormat { .. })
            ));
        }
        for usage in [
            wgpu::TextureUsages::TEXTURE_BINDING,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::STORAGE_BINDING,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TRANSIENT_ATTACHMENT,
        ] {
            assert!(matches!(
                graphics.create_framebuffer(
                    FramebufferOptions::new(64, 64)
                        .depth_stencil(wgpu::TextureFormat::Depth24Plus)
                        .depth_stencil_usage(usage)
                ),
                Err(Error::InvalidDepthStencilUsage { .. })
            ));
        }
        let mut offscreen = target(&graphics, wgpu::TextureFormat::Depth24PlusStencil8, 1);
        let supported = offscreen.supported_sample_counts().as_ptr();
        let texture = offscreen.depth_stencil_texture().unwrap().unwrap().clone();
        assert_eq!(texture.size().width, 64);
        assert_eq!(texture.sample_count(), 1);
        assert_eq!(
            offscreen.depth_stencil_usage(),
            Some(wgpu::TextureUsages::RENDER_ATTACHMENT)
        );
        offscreen.resize(64, 64).unwrap();
        offscreen.set_sample_count(1).unwrap();
        assert_eq!(
            offscreen.depth_stencil_texture().unwrap().unwrap(),
            &texture
        );
        for invalid in [0, 3, u32::MAX] {
            assert!(matches!(
                offscreen.set_sample_count(invalid),
                Err(Error::UnsupportedSampleCount { .. })
            ));
            assert_eq!(
                offscreen.depth_stencil_texture().unwrap().unwrap(),
                &texture
            );
        }
        offscreen.set_sample_count(4).unwrap();
        assert_eq!(
            offscreen
                .depth_stencil_texture()
                .unwrap()
                .unwrap()
                .sample_count(),
            4
        );
        assert_ne!(
            offscreen.depth_stencil_texture().unwrap().unwrap(),
            &texture
        );
        let mut frame = offscreen.begin_frame().unwrap();
        assert!(matches!(
            frame.render_pass().load_depth().begin(),
            Err(Error::UninitializedDepth)
        ));
        drop(frame.render_pass().begin().unwrap());
        frame.finish().unwrap();
        offscreen.resize(32, 48).unwrap();
        assert_eq!(
            offscreen.depth_stencil_texture().unwrap().unwrap().width(),
            32
        );
        assert_eq!(
            offscreen.depth_stencil_texture().unwrap().unwrap().height(),
            48
        );
        assert_eq!(supported, offscreen.supported_sample_counts().as_ptr());
        offscreen.resize(0, 0).unwrap();
        assert!(matches!(
            offscreen.depth_stencil_texture(),
            Err(Error::TargetSuspended)
        ));
        assert!(matches!(
            offscreen.depth_stencil_view(),
            Err(Error::TargetSuspended)
        ));
        offscreen.resize(64, 64).unwrap();

        // Pending writes to the old framebuffer generation cannot initialize its replacement.
        let mut base = graphics
            .create_framebuffer(FramebufferOptions::new(64, 64))
            .unwrap();
        let mut frame = base.begin_frame().unwrap();
        drop(frame.render_to(&mut offscreen).begin().unwrap());
        offscreen.resize(32, 32).unwrap();
        frame.finish().unwrap();
        let mut fresh = offscreen.begin_frame().unwrap();
        assert!(matches!(
            fresh.render_pass().load_depth().begin(),
            Err(Error::UninitializedDepth)
        ));
        assert!(matches!(
            fresh.render_pass().load_stencil().begin(),
            Err(Error::UninitializedStencil)
        ));
        drop(fresh);
        let mut generic: RenderTarget<'static> = offscreen.into();
        assert_eq!(
            generic.depth_stencil_format(),
            Some(wgpu::TextureFormat::Depth24PlusStencil8)
        );
        assert!(generic.depth_stencil_view().unwrap().is_some());
        let mut renderer = MeshRenderer::new(&graphics);
        renderer.prepare_for_target(&generic).unwrap();
        let material = material(
            &graphics,
            &renderer,
            state(generic.depth_stencil_format().unwrap()),
        );
        renderer
            .prepare_material_for_target(&material, &generic)
            .unwrap();
        let cached = renderer.pipelines.clone();
        let mesh = quad(&graphics, 0.0, 0.5, 0.0, [1.0; 4]);
        let mut frame = generic.begin_frame().unwrap();
        let mut pass = frame.render_pass().begin().unwrap();
        renderer.draw(&mut pass, &mesh).unwrap();
        renderer
            .draw_with_material(&mut pass, &mesh, &material)
            .unwrap();
        drop(pass);
        frame.finish().unwrap();
        assert_eq!(renderer.pipelines, cached);
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
        let foreign: RenderTarget<'static> = other
            .create_framebuffer(FramebufferOptions::new(64, 64))
            .unwrap()
            .into();
        assert!(matches!(
            renderer.prepare_for_target(&foreign),
            Err(Error::DeviceMismatch)
        ));
        assert!(matches!(
            renderer.prepare_material_for_target(&material, &foreign),
            Err(Error::DeviceMismatch)
        ));
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn stencil_write_masks_preserve_independent_bit_flags() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let format = wgpu::TextureFormat::Stencil8;
        let mut target = target(&graphics, format, 1);
        let mut renderer = MeshRenderer::new(&graphics);
        let flag_material = |mask| {
            let mut state = state(format);
            let face = wgpu::StencilFaceState {
                compare: wgpu::CompareFunction::Always,
                pass_op: wgpu::StencilOperation::Replace,
                ..Default::default()
            };
            state.stencil = wgpu::StencilState {
                front: face,
                back: face,
                read_mask: 0xff,
                write_mask: mask,
            };
            graphics.create_material(
                MaterialOptions::new(renderer.default_material().shader())
                    .write_mask(wgpu::ColorWrites::empty())
                    .depth_stencil(Some(state)),
            )
        };
        let first_bit = flag_material(0b01);
        let second_bit = flag_material(0b10);
        let mut test_state = state(format);
        let face = wgpu::StencilFaceState {
            compare: wgpu::CompareFunction::Equal,
            ..Default::default()
        };
        test_state.stencil = wgpu::StencilState {
            front: face,
            back: face,
            read_mask: 0b11,
            write_mask: 0,
        };
        let both_bits = material(&graphics, &renderer, test_state);
        let left = quad(&graphics, -0.2, 0.5, 0.0, [1.0; 4]);
        let right = quad(&graphics, 0.2, 0.5, 0.0, [1.0; 4]);
        let green = quad(&graphics, 0.0, 0.9, 0.0, [0.0, 1.0, 0.0, 1.0]);
        let image = pixels(&graphics, &mut target, |frame| {
            let mut pass = frame
                .render_pass()
                .clear_color(wgpu::Color::BLACK)
                .stencil_reference(0b01)
                .begin()
                .unwrap();
            renderer
                .draw_with_material(&mut pass, &left, &first_bit)
                .unwrap();
            pass.set_stencil_reference(0b10);
            renderer
                .draw_with_material(&mut pass, &right, &second_bit)
                .unwrap();
            pass.set_stencil_reference(0b11);
            renderer
                .draw_with_material(&mut pass, &green, &both_bits)
                .unwrap();
        });
        assert_eq!(
            at(&image, 16),
            [0, 0, 0, 255],
            "first flag alone fails the combined test"
        );
        assert_eq!(
            at(&image, 48),
            [0, 0, 0, 255],
            "second flag alone fails the combined test"
        );
        assert_eq!(
            at(&image, 32),
            [0, 255, 0, 255],
            "masked writes preserve both bits in overlap"
        );
    });
}
