use super::*;
use crate::TextRasterOptions;
use crate::framebuffer::tests::pixels;
use crate::{
    DrawSpace, Framebuffer, FramebufferOptions, LineCap, MeshRenderer, TextureOptions, Vertex,
};
fn target(g: &GraphicsContext) -> Framebuffer {
    g.create_framebuffer(FramebufferOptions::new(64, 64).usage(
        wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::TEXTURE_BINDING,
    ))
    .unwrap()
}
fn pixel(p: &[u8], x: usize, y: usize) -> [u8; 4] {
    p[(y * 64 + x) * 4..(y * 64 + x + 1) * 4]
        .try_into()
        .unwrap()
}
const RED: [f32; 4] = [1., 0., 0., 1.];
const GREEN: [f32; 4] = [0., 1., 0., 1.];
const BLUE: [f32; 4] = [0., 0., 1., 1.];

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn painter_matches_direct_renderers_with_custom_pass_access() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut t = target(&g);
        let mut painter = Painter::new(&g);
        let mut shapes = ShapeRenderer::new(&g);
        let mut lines = LineRenderer::new(&g);
        let mut textures = TextureRenderer::new(&g);
        let mut meshes = MeshRenderer::new(&g);
        let texture = g.create_texture(TextureOptions::new(1, 1)).unwrap();
        texture.write(&[255; 4]).unwrap();
        // A binding from an independent renderer is usable by the facade.
        let image = textures
            .create_binding(texture.view(), TextureBindingOptions::new())
            .unwrap();
        painter.prepare(&t.render_format()).unwrap();
        painter.prepare_image(&image, &t.render_format()).unwrap();
        let mesh = g
            .create_mesh(
                &[
                    Vertex::new([-1., -1., 0.], BLUE),
                    Vertex::new([1., -1., 0.], BLUE),
                    Vertex::new([0., 1., 0.], BLUE),
                ],
                &[0, 1, 2],
            )
            .unwrap();
        let placement = TextureDraw::new(Rect::new(8., 8., 24., 24.)).tint([0., 0., 1., 0.5]);
        let ellipse = ShapeDraw::ellipse(Rect::new(16., 16., 24., 24.), [0., 1., 0., 0.5]);
        let line = LineDraw::new([4., 44.], [60., 44.], GREEN)
            .width(4.)
            .cap(LineCap::Round);
        let direct = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            shapes
                .draw(&mut p, ShapeDraw::rect(Rect::new(0., 0., 64., 64.), RED))
                .unwrap();
            textures.draw(&mut p, &image, placement).unwrap();
            shapes.draw(&mut p, ellipse).unwrap();
            p.set_scissor_rect(0, 0, 32, 64).unwrap();
            meshes.draw(&mut p, &mesh).unwrap();
            p.as_wgpu().set_scissor_rect(0, 0, 1, 1);
            lines.draw(&mut p, line).unwrap();
            p.set_scissor_rect(0, 0, 64, 64).unwrap();
            shapes
                .draw(
                    &mut p,
                    ShapeDraw::rounded_rect(Rect::new(48., 4., 12., 12.), 3., GREEN),
                )
                .unwrap();
        });
        let painted = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            {
                let mut paint = painter.begin(&mut p).unwrap();
                paint.fill_rect(Rect::new(0., 0., 64., 64.), RED).unwrap();
                paint.draw_image(&image, placement).unwrap();
                paint.draw_shape(ellipse).unwrap();
                paint.pass().set_scissor_rect(0, 0, 32, 64).unwrap();
                meshes.draw(paint.pass(), &mesh).unwrap();
                paint.pass().as_wgpu().set_scissor_rect(0, 0, 1, 1);
                paint.draw_line(line).unwrap();
            }
            // Session drop leaves the same pass available, with no pending commands.
            p.set_scissor_rect(0, 0, 64, 64).unwrap();
            shapes
                .draw(
                    &mut p,
                    ShapeDraw::rounded_rect(Rect::new(48., 4., 12., 12.), 3., GREEN),
                )
                .unwrap();
        });
        assert_eq!(direct, painted);
        assert_eq!(pixel(&painted, 54, 10), [0, 255, 0, 255]);
        assert_eq!(pixel(&painted, 20, 44), [0, 255, 0, 255]);
        assert_eq!(pixel(&painted, 50, 44), [255, 0, 0, 255]);
        assert!(errors.pop().await.is_none());
    });
}
#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn nested_transform_scopes_preserve_parent_state_and_coordinate_units() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let mut t = target(&g);
        let mut painter = Painter::new(&g);
        let texture = g.create_texture(TextureOptions::new(1, 1)).unwrap();
        texture.write(&[255; 4]).unwrap();
        let image = painter
            .create_image_binding(texture.view(), TextureBindingOptions::new())
            .unwrap();
        let bytes = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            let mut paint = painter.begin(&mut p).unwrap();
            paint.fill_rect(Rect::new(0., 0., 8., 8.), RED).unwrap();
            {
                let mut parent = paint
                    .transformed(Transform2D::translation(16., 4.))
                    .unwrap();
                parent
                    .fill_rect(Rect::new(0., 0., 12., 16.), GREEN)
                    .unwrap();
                {
                    let mut child = parent.transformed(Transform2D::scale(2., 2.)).unwrap();
                    child.fill_rect(Rect::new(1., 1., 4., 4.), BLUE).unwrap();
                }
                assert_eq!(parent.transform(), Transform2D::translation(16., 4.));
                parent
                    .draw_image(&image, TextureDraw::new(Rect::new(8., 12., 4., 4.)))
                    .unwrap();
                parent
                    .draw_line(LineDraw::new([0., 22.], [12., 22.], GREEN).width(4.))
                    .unwrap();
            }
            assert_eq!(paint.transform(), Transform2D::IDENTITY);
            paint
                .draw_shape(
                    ShapeDraw::rect(Rect::new(0.75, 0.75, 0.125, 0.125), RED)
                        .space(DrawSpace::Normalized),
                )
                .unwrap();
        });
        assert_eq!(pixel(&bytes, 4, 4), [255, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 16, 5), [0, 255, 0, 255]);
        assert_eq!(pixel(&bytes, 20, 8), [0, 0, 255, 255]);
        assert_eq!(pixel(&bytes, 25, 17), [255; 4]);
        assert_eq!(pixel(&bytes, 20, 26), [0, 255, 0, 255]);
        assert_eq!(pixel(&bytes, 50, 50), [255, 0, 0, 255]);
    });
}
#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn transformed_batches_match_individual_painting_and_reuse_scratch() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let mut t = target(&g);
        let mut painter = Painter::new(&g);
        let texture = g.create_texture(TextureOptions::new(1, 1)).unwrap();
        texture.write(&[255; 4]).unwrap();
        let image = painter
            .create_image_binding(texture.view(), TextureBindingOptions::new())
            .unwrap();
        let shapes = [
            ShapeDraw::rect(Rect::new(0., 0., 8., 8.), RED),
            ShapeDraw::ellipse(Rect::new(8., 0., 8., 8.), GREEN).stroke(Stroke::new(1.).inside()),
        ];
        let lines = [LineDraw::new([0., 12.], [16., 12.], BLUE).width(2.)];
        let images = [
            TextureDraw::new(Rect::new(0., 16., 8., 8.)),
            TextureDraw::new(Rect::new(8., 16., 8., 8.)).tint([0., 1., 1., 0.5]),
        ];
        let render = |f: &mut crate::Frame<'_, '_>, painter: &mut Painter, batch: bool| {
            let mut p = f.render_pass().begin().unwrap();
            let mut paint = painter.begin(&mut p).unwrap();
            let mut child = paint
                .transformed(Transform2D::scale(2., 2.).then(Transform2D::translation(4., 4.)))
                .unwrap();
            if batch {
                child.draw_shapes(&shapes).unwrap();
                child.draw_lines(&lines).unwrap();
                child.draw_images(&image, &images).unwrap();
            } else {
                for &d in &shapes {
                    child.draw_shape(d).unwrap();
                }
                for &d in &lines {
                    child.draw_line(d).unwrap();
                }
                for &d in &images {
                    child.draw_image(&image, d).unwrap();
                }
            }
        };
        let individual = pixels(&g, &mut t, |f| render(f, &mut painter, false));
        let batched = pixels(&g, &mut t, |f| render(f, &mut painter, true));
        assert_eq!(individual, batched);
        let capacities = [
            painter.shape_scratch.capacity(),
            painter.line_scratch.capacity(),
            painter.image_scratch.capacity(),
        ];
        let buffers: Vec<_> = g
            .upload_pool
            .lock()
            .unwrap()
            .iter()
            .map(|(b, _)| b.clone())
            .collect();
        pixels(&g, &mut t, |f| render(f, &mut painter, true));
        assert_eq!(
            [
                painter.shape_scratch.capacity(),
                painter.line_scratch.capacity(),
                painter.image_scratch.capacity()
            ],
            capacities
        );
        assert!(
            g.upload_pool
                .lock()
                .unwrap()
                .iter()
                .all(|(b, _)| buffers.contains(b))
        );
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn painter_outline_helpers_match_direct_shapes_under_affine_transforms() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let mut t = target(&g);
        let mut painter = Painter::new(&g);
        let mut shapes = ShapeRenderer::new(&g);
        let transform = Transform2D([1.2, 0.2, 0.3, 0.8, 4., 6.]);
        let rect = Rect::new(2., 2., 18., 14.);
        let rounded = Rect::new(22., 2., 16., 16.);
        let ellipse = Rect::new(8., 26., 28., 14.);
        let direct = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            shapes
                .draw_many(
                    &mut p,
                    &[
                        ShapeDraw::rect(rect, RED)
                            .stroke(Stroke::new(2.).inside())
                            .transform(transform),
                        ShapeDraw::rounded_rect(rounded, 5., GREEN)
                            .stroke(Stroke::new(2.))
                            .transform(transform),
                        ShapeDraw::ellipse(ellipse, BLUE)
                            .stroke(Stroke::new(2.).outside())
                            .transform(transform),
                    ],
                )
                .unwrap();
        });
        let painted = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            let mut paint = painter.begin(&mut p).unwrap();
            let mut local = paint.transformed(transform).unwrap();
            local
                .stroke_rect(rect, Stroke::new(2.).inside(), RED)
                .unwrap();
            local
                .stroke_rounded_rect(rounded, 5., Stroke::new(2.), GREEN)
                .unwrap();
            local
                .stroke_ellipse(ellipse, Stroke::new(2.).outside(), BLUE)
                .unwrap();
        });
        assert_eq!(direct, painted);
        assert!(painted.iter().any(|&byte| byte != 0));
    });
}
#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn invalid_children_batches_devices_and_feedback_leave_session_usable() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let foreign = GraphicsContext::headless().await.unwrap();
        let mut t = target(&g);
        let mut painter = Painter::new(&g);
        let mut foreign_painter = Painter::new(&foreign);
        let image = g.create_texture(TextureOptions::new(1, 1)).unwrap();
        image.write(&[255; 4]).unwrap();
        let binding = painter
            .create_image_binding(image.view(), TextureBindingOptions::new())
            .unwrap();
        let foreign_image = foreign.create_texture(TextureOptions::new(1, 1)).unwrap();
        let foreign_binding = foreign_painter
            .create_image_binding(foreign_image.view(), TextureBindingOptions::new())
            .unwrap();
        let feedback = painter.create_sampled_binding(&t.sampled_color()).unwrap();
        let bytes = pixels(&g, &mut t, |f| {
            let mut p = f
                .render_pass()
                .clear_color(wgpu::Color::BLUE)
                .begin()
                .unwrap();
            assert!(matches!(
                foreign_painter.begin(&mut p),
                Err(Error::DeviceMismatch)
            ));
            let mut paint = painter.begin(&mut p).unwrap();
            assert!(matches!(
                paint.transformed([f32::INFINITY; 6]),
                Err(Error::InvalidTransform2D)
            ));
            assert_eq!(paint.transform(), Transform2D::IDENTITY);
            assert!(matches!(
                paint.draw_image(&feedback, TextureDraw::default()),
                Err(Error::TextureFeedback)
            ));
            assert!(matches!(
                paint.draw_image(&foreign_binding, TextureDraw::default()),
                Err(Error::DeviceMismatch)
            ));
            {
                let mut child = paint.transformed(Transform2D::translation(1., 1.)).unwrap();
                assert!(matches!(
                    child.draw_shapes(&[
                        ShapeDraw::rect(Rect::new(0., 0., 64., 64.), RED),
                        ShapeDraw::ellipse(Rect::new(0., 0., -1., 1.), RED)
                    ]),
                    Err(Error::InvalidShapeDraw)
                ));
                assert!(matches!(
                    child.draw_lines(&[
                        LineDraw::new([0., 32.], [64., 32.], RED).width(64.),
                        LineDraw::new([0.; 2], [1.; 2], RED).width(-1.)
                    ]),
                    Err(Error::InvalidLineDraw)
                ));
                assert!(matches!(
                    child.draw_images(
                        &binding,
                        &[
                            TextureDraw::default().tint(RED),
                            TextureDraw::new(Rect::new(0., 0., -1., 1.))
                        ]
                    ),
                    Err(Error::InvalidTextureDraw)
                ));
            }
            paint.fill_rect(Rect::new(0., 0., 8., 8.), GREEN).unwrap();
        });
        assert_eq!(pixel(&bytes, 32, 32), [0, 0, 255, 255]);
        assert_eq!(pixel(&bytes, 4, 4), [0, 255, 0, 255]);
    });
}

fn text_layout(content: &str) -> std::sync::Arc<TextLayout> {
    let mut fonts = crate::TextSystem::new();
    fonts
        .load_font(include_bytes!("../tests/fonts/TestColor.ttf"))
        .unwrap();
    let mut buffer = crate::TextBuffer::new();
    buffer
        .set_text(
            content,
            crate::TextStyle::new()
                .family("RXUI drawing Test Color")
                .font_size(20.)
                .line_height(24.),
        )
        .unwrap();
    buffer.layout(&mut fonts).unwrap()
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn painter_text_matches_direct_layers_nested_transforms_and_pass_state() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        for samples in [1, 4] {
            let mut t = g
                .create_framebuffer(
                    FramebufferOptions::new(64, 64)
                        .sample_count(samples)
                        .depth_stencil(wgpu::TextureFormat::Depth24PlusStencil8)
                        .usage(
                            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                        ),
                )
                .unwrap();
            let mut painter = Painter::new(&g);
            let mut text = TextRenderer::new(&g);
            let mut shapes = ShapeRenderer::new(&g);
            let mut lines = LineRenderer::new(&g);
            let mut meshes = MeshRenderer::new(&g);
            let texture = g.create_texture(TextureOptions::new(1, 1)).unwrap();
            texture.write(&[255; 4]).unwrap();
            let mut images = TextureRenderer::new(&g);
            let image = images
                .create_binding(texture.view(), TextureBindingOptions::new())
                .unwrap();
            let mesh = g
                .create_mesh(
                    &[
                        Vertex::new([-1., -1., 0.], BLUE),
                        Vertex::new([1., -1., 0.], BLUE),
                        Vertex::new([0., 1., 0.], BLUE),
                    ],
                    &[0, 1, 2],
                )
                .unwrap();
            painter.prepare(&t.render_format()).unwrap();
            painter.prepare_image(&image, &t.render_format()).unwrap();
            // Two separately prepared resources can be drawn by either renderer.
            let mask = painter
                .prepare_text(&text_layout("M"), TextRasterOptions::new().raster_scale(2.))
                .unwrap();
            let colored = text
                .prepare_text(&text_layout("😀😁"), TextRasterOptions::new())
                .unwrap();
            let parent = Transform2D::translation(8., 2.);
            let child = Transform2D::scale(0.75, 0.75);
            let draw = TextDraw::new([2., 2.])
                .color([1., 0., 0., 0.8])
                .opacity(0.8)
                .transform(Transform2D::translation(1., 0.));
            let overlay = ShapeDraw::rect(Rect::new(14., 10., 8., 8.), [0., 1., 0., 0.5]);
            let line = LineDraw::new([0., 20.], [40., 20.], GREEN).width(2.);
            let direct = pixels(&g, &mut t, |f| {
                let mut p = f.render_pass().begin().unwrap();
                p.set_viewport(4., 4., 56., 56., 0., 1.).unwrap();
                p.set_scissor_rect(4, 4, 50, 50).unwrap();
                images
                    .draw(
                        &mut p,
                        &image,
                        TextureDraw::default().tint([0.05, 0.05, 0.05, 1.]),
                    )
                    .unwrap();
                text.draw(
                    &mut p,
                    &mask,
                    TextDraw {
                        transform: draw.transform.then(child).then(parent),
                        ..draw
                    },
                )
                .unwrap();
                shapes.draw(&mut p, overlay).unwrap();
                meshes.draw(&mut p, &mesh).unwrap();
                // A raw state mutation must be restored by the following text draw.
                p.as_wgpu().set_scissor_rect(0, 0, 1, 1);
                text.draw(
                    &mut p,
                    &colored,
                    TextDraw::new([0., 30.]).opacity(0.7).transform(parent),
                )
                .unwrap();
                lines
                    .draw(&mut p, line.transform(child.then(parent)))
                    .unwrap();
                text.draw(&mut p, &mask, TextDraw::new([44., 0.]).color(GREEN))
                    .unwrap();
            });
            let before = painter.text().stats();
            let painted = pixels(&g, &mut t, |f| {
                let mut p = f.render_pass().begin().unwrap();
                let mut paint = painter.begin(&mut p).unwrap();
                paint.pass().set_viewport(4., 4., 56., 56., 0., 1.).unwrap();
                paint.pass().set_scissor_rect(4, 4, 50, 50).unwrap();
                paint
                    .draw_image(&image, TextureDraw::default().tint([0.05, 0.05, 0.05, 1.]))
                    .unwrap();
                {
                    let mut local = paint.transformed(parent).unwrap();
                    {
                        let mut nested = local.transformed(child).unwrap();
                        nested.draw_text(&mask, draw).unwrap();
                    }
                    // Pass access bypasses the Painter transform, including custom renderers.
                    shapes.draw(local.pass(), overlay).unwrap();
                    meshes.draw(local.pass(), &mesh).unwrap();
                    local.pass().as_wgpu().set_scissor_rect(0, 0, 1, 1);
                    local
                        .draw_text(&colored, TextDraw::new([0., 30.]).opacity(0.7))
                        .unwrap();
                    local.transformed(child).unwrap().draw_line(line).unwrap();
                }
                assert_eq!(paint.transform(), Transform2D::IDENTITY);
                paint
                    .draw_text(&mask, TextDraw::new([44., 0.]).color(GREEN))
                    .unwrap();
            });
            assert_eq!(direct, painted);
            assert_eq!(pixel(&painted, 0, 0), [0; 4]);
            assert!(
                painted
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[0] > 30 && p[0] > p[2])
            );
            assert!(
                painted
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[1] > 100 && p[1] > p[2])
            );
            let after = painter.text().stats();
            assert_eq!(before.cache_misses, after.cache_misses);
            assert_eq!(before.uploaded_bytes, after.uploaded_bytes);
            assert_eq!(before.geometry_bytes, after.geometry_bytes);
            assert_eq!(after.parameter_bytes - before.parameter_bytes, 3 * 48);
            assert_eq!(after.draw_calls - before.draw_calls, 3);
        }
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn rejected_painter_text_draws_leave_session_and_cached_resources_usable() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let foreign = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut t = target(&g);
        let mut painter = Painter::new(&g);
        let layout = text_layout("M");
        let mut other = TextRenderer::new(&foreign);
        let foreign_text = other
            .prepare_text(&layout, TextRasterOptions::new())
            .unwrap();
        let text = painter
            .prepare_text(&layout, TextRasterOptions::new())
            .unwrap();
        painter.prepare(&t.render_format()).unwrap();
        painter.text().clear_cache();
        assert_eq!(painter.text().stats().live_pages, 1);
        let before = painter.text().stats();
        let bytes = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            let mut paint = painter.begin(&mut p).unwrap();
            assert!(matches!(
                paint.draw_text(&foreign_text, TextDraw::default()),
                Err(Error::DeviceMismatch)
            ));
            assert!(matches!(
                paint.draw_text(&text, TextDraw::default().opacity(2.)),
                Err(Error::InvalidTextDraw)
            ));
            {
                let mut local = paint
                    .transformed(Transform2D::scale(f32::MAX, f32::MAX))
                    .unwrap();
                assert!(matches!(
                    local.draw_text(
                        &text,
                        TextDraw::default().transform(Transform2D::scale(2., 2.))
                    ),
                    Err(Error::InvalidTextDraw)
                ));
            }
            paint
                .draw_text(&text, TextDraw::new([24., 0.]).color(GREEN))
                .unwrap();
        });
        assert_eq!(pixel(&bytes, 5, 10), [0; 4]);
        assert_eq!(pixel(&bytes, 29, 10), [0, 255, 0, 255]);
        let after = painter.text().stats();
        assert_eq!(after.parameter_bytes - before.parameter_bytes, 48);
        assert_eq!(after.draw_calls - before.draw_calls, 1);
        assert_eq!(after.uploaded_bytes, before.uploaded_bytes);
        assert_eq!(after.geometry_bytes, before.geometry_bytes);
        drop(text);
        assert_eq!(painter.text().stats().live_pages, 0);
        assert!(errors.pop().await.is_none());
    });
}

fn stencil_state(compare: wgpu::CompareFunction, write: bool) -> wgpu::DepthStencilState {
    let face = wgpu::StencilFaceState {
        compare,
        fail_op: wgpu::StencilOperation::Keep,
        depth_fail_op: wgpu::StencilOperation::Keep,
        pass_op: if write {
            wgpu::StencilOperation::Replace
        } else {
            wgpu::StencilOperation::Keep
        },
    };
    wgpu::DepthStencilState {
        format: wgpu::TextureFormat::Depth24PlusStencil8,
        depth_write_enabled: Some(false),
        depth_compare: None,
        stencil: wgpu::StencilState {
            front: face,
            back: face,
            read_mask: 0xff,
            write_mask: if write { 0xff } else { 0 },
        },
        bias: Default::default(),
    }
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn stencil_clips_all_painter_renderers_and_reference_changes_take_effect() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let white = g.create_texture(TextureOptions::new(1, 1)).unwrap();
        white.write(&[255; 4]).unwrap();
        let policy = crate::PipelineOptions::new()
            .depth_stencil(Some(stencil_state(wgpu::CompareFunction::Equal, false)));
        let mut painter = Painter::with_options(&g, policy);
        let mut mask = ShapeRenderer::with_options(
            &g,
            crate::PipelineOptions::new()
                .write_mask(wgpu::ColorWrites::empty())
                .depth_stencil(Some(stencil_state(wgpu::CompareFunction::Always, true))),
        );
        let image = painter
            .create_image_binding(white.view(), TextureBindingOptions::new())
            .unwrap();
        let text = painter
            .prepare_text(&text_layout("M😀M"), TextRasterOptions::new())
            .unwrap();
        let field = painter
            .prepare_text(&text_layout("M😀M"), crate::MtsdfOptions::new())
            .unwrap();
        for samples in [1, 4] {
            let mut t = g
                .create_framebuffer(
                    FramebufferOptions::new(64, 64)
                        .sample_count(samples)
                        .depth_stencil(wgpu::TextureFormat::Depth24PlusStencil8)
                        .usage(
                            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                        ),
                )
                .unwrap();
            painter.prepare(&t.render_format()).unwrap();
            painter.prepare_image(&image, &t.render_format()).unwrap();
            mask.prepare(&t.render_format()).unwrap();
            let prepared = painter
                .prepare_images(
                    &[TextureDraw::new(Rect::new(0., 52., 64., 12.))],
                    [64., 64.],
                )
                .unwrap();
            for text in [&text, &field] {
                let bytes = pixels(&g, &mut t, |f| {
                    {
                        let mut p = f.render_pass().begin().unwrap();
                        p.set_stencil_reference(1);
                        mask.draw(
                            &mut p,
                            ShapeDraw::rect(Rect::new(0., 0., 32., 64.), [1.; 4])
                                .antialiasing(crate::EdgeAntialiasing::None),
                        )
                        .unwrap();
                    }
                    let mut p = f
                        .render_pass()
                        .load_all()
                        .depth_ops(None)
                        .stencil_ops(None)
                        .begin()
                        .unwrap();
                    p.set_stencil_reference(1);
                    let mut paint = painter.begin(&mut p).unwrap();
                    paint.fill_rect(Rect::new(0., 0., 64., 12.), RED).unwrap();
                    paint
                        .draw_line(LineDraw::new([0., 20.], [64., 20.], GREEN).width(8.))
                        .unwrap();
                    paint.draw_text(text, TextDraw::new([0., 28.])).unwrap();
                    paint.draw_prepared_images(&image, &prepared).unwrap();
                    paint.pass().set_stencil_reference(2);
                    paint.fill_rect(Rect::new(0., 0., 64., 64.), BLUE).unwrap();
                    // Read-only passes reject writing policies before recording a draw.
                    assert!(matches!(
                        mask.draw(
                            paint.pass(),
                            ShapeDraw::rect(Rect::new(0., 0., 64., 64.), RED)
                        ),
                        Err(Error::ReadOnlyStencil)
                    ));
                    paint.pass().set_stencil_reference(1);
                    paint.fill_rect(Rect::new(4., 4., 4., 4.), GREEN).unwrap();
                });
                for y in 0..64 {
                    for x in 32..64 {
                        assert_eq!(pixel(&bytes, x, y), [0; 4]);
                    }
                }
                assert_eq!(pixel(&bytes, 6, 6), [0, 255, 0, 255]);
                assert_eq!(pixel(&bytes, 10, 20), [0, 255, 0, 255]);
                assert_eq!(pixel(&bytes, 10, 56), [255; 4]);
                assert!((28..52).any(|y| (0..32).any(|x| pixel(&bytes, x, y)[3] > 0)));
            }
            let mut f = t.begin_frame().unwrap();
            let mut p = f.render_pass().without_depth_stencil().begin().unwrap();
            let mut paint = painter.begin(&mut p).unwrap();
            assert!(matches!(
                paint.fill_rect(Rect::new(0., 0., 10., 10.), RED),
                Err(Error::DepthStencilMismatch { .. })
            ));
            assert!(matches!(
                paint.draw_text(&text, TextDraw::default()),
                Err(Error::DepthStencilMismatch { .. })
            ));
            assert!(matches!(
                paint.draw_image(&image, TextureDraw::default()),
                Err(Error::DepthStencilMismatch { .. })
            ));
        }
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn prepared_images_match_dynamic_placements_under_nested_transforms_and_pass_access() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let texture = g.create_texture(TextureOptions::new(1, 1)).unwrap();
        texture.write(&[255; 4]).unwrap();
        let mut painter = Painter::new(&g);
        let image = painter
            .create_image_binding(texture.view(), TextureBindingOptions::new())
            .unwrap();
        let draws = [
            TextureDraw::new(Rect::new(2., 2., 12., 8.))
                .tint(RED)
                .transform(Transform2D::translation(1., 2.)),
            TextureDraw::normalized(Rect::new(0.1, 0.3, 0.3, 0.2)).tint(GREEN),
        ];
        let text = painter
            .prepare_text(&text_layout("M"), TextRasterOptions::new())
            .unwrap();
        for samples in [1, 4] {
            let mut t = g
                .create_framebuffer(
                    FramebufferOptions::new(64, 64).sample_count(samples).usage(
                        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                    ),
                )
                .unwrap();
            painter.prepare(&t.render_format()).unwrap();
            painter.prepare_image(&image, &t.render_format()).unwrap();
            let prepared = painter.prepare_images(&draws, [56., 48.]).unwrap();
            for transform in [
                Transform2D::IDENTITY,
                Transform2D([1.2, 0.1, -0.15, 0.8, 0.1, 0.2]),
            ] {
                let render =
                    |f: &mut crate::Frame<'_, '_>, painter: &mut Painter, retained: bool| {
                        let mut p = f.render_pass().begin().unwrap();
                        p.set_viewport(4., 8., 56., 48., 0., 1.).unwrap();
                        p.set_scissor_rect(4, 8, 52, 44).unwrap();
                        let mut paint = painter.begin(&mut p).unwrap();
                        let mut child = paint.transformed(transform).unwrap();
                        if retained {
                            child.draw_prepared_images(&image, &prepared).unwrap();
                        } else {
                            child.draw_images(&image, &draws).unwrap();
                        }
                        child
                            .draw_text(&text, TextDraw::new([20., 20.]).color(BLUE))
                            .unwrap();
                        child.pass().as_wgpu().set_scissor_rect(0, 0, 1, 1);
                        // Both raw and other renderer state changes must be restored.
                        if retained {
                            child.draw_prepared_images(&image, &prepared).unwrap();
                        } else {
                            child.draw_images(&image, &draws).unwrap();
                        }
                        child.fill_rect(Rect::new(25., 0., 4., 4.), BLUE).unwrap();
                    };
                let dynamic = pixels(&g, &mut t, |f| render(f, &mut painter, false));
                let retained = pixels(&g, &mut t, |f| render(f, &mut painter, true));
                // Separate CPU and shader composition can differ by one rounding bit.
                for (a, b) in dynamic.iter().zip(&retained) {
                    assert!(a.abs_diff(*b) <= 1);
                }
                assert!(retained.iter().any(|&v| v != 0));
            }
            pixels(&g, &mut t, |f| {
                let mut p = f.render_pass().begin().unwrap();
                let mut paint = painter.begin(&mut p).unwrap();
                assert!(matches!(
                    paint.draw_prepared_images(&image, &prepared),
                    Err(Error::InvalidTextureDraw)
                ));
                paint.pass().set_viewport(0., 0., 56., 48., 0., 1.).unwrap();
                let mut child = paint
                    .transformed(Transform2D::translation(f32::MAX, f32::MAX))
                    .unwrap();
                assert!(child.draw_prepared_images(&image, &prepared).is_err());
            });
        }
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn raster_density_does_not_double_scale_text_in_logical_painter_sessions() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let mut painter = Painter::new(&g);
        let layout = text_layout("M");
        let text = painter
            .prepare_text(&layout, TextRasterOptions::new().raster_scale(2.))
            .unwrap();
        assert_eq!(text.size(), layout.size());
        let mut t = target(&g);
        let bytes = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            let mut paint = painter.begin(&mut p).unwrap();
            let mut logical = paint.transformed(Transform2D::scale(2., 2.)).unwrap();
            logical
                .fill_rect(Rect::new(2., 2., 20., 20.), GREEN)
                .unwrap();
            logical
                .draw_text(&text, TextDraw::new([2., 2.]).color(RED))
                .unwrap();
        });
        assert_eq!(pixel(&bytes, 15, 15), [255, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 45, 15), [0; 4]);
        assert_eq!(pixel(&bytes, 15, 50), [0; 4]);
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn builtin_stencil_writes_follow_visible_coverage_and_readonly_passes_reject_writers() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut t = g
            .create_framebuffer(
                FramebufferOptions::new(64, 64)
                    .depth_stencil(wgpu::TextureFormat::Depth24PlusStencil8)
                    .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC),
            )
            .unwrap();
        let mut writer = Painter::with_options(
            &g,
            crate::PipelineOptions::new()
                .write_mask(wgpu::ColorWrites::empty())
                .depth_stencil(Some(stencil_state(wgpu::CompareFunction::Always, true))),
        );
        let mut reader = Painter::with_options(
            &g,
            crate::PipelineOptions::new()
                .depth_stencil(Some(stencil_state(wgpu::CompareFunction::Equal, false))),
        );
        let texture = g.create_texture(TextureOptions::new(2, 1)).unwrap();
        texture.write(&[255, 255, 255, 255, 0, 0, 0, 0]).unwrap();
        let image = writer
            .create_image_binding(texture.view(), TextureBindingOptions::new())
            .unwrap();
        let text = writer
            .prepare_text(&text_layout("M"), TextRasterOptions::new())
            .unwrap();
        let bytes = pixels(&g, &mut t, |f| {
            {
                let mut p = f.render_pass().begin().unwrap();
                p.set_stencil_reference(1);
                let mut paint = writer.begin(&mut p).unwrap();
                paint
                    .draw_shape(
                        ShapeDraw::rounded_rect(Rect::new(4., 4., 20., 20.), 8., [1.; 4])
                            .antialiasing(crate::EdgeAntialiasing::None),
                    )
                    .unwrap();
                paint
                    .draw_line(
                        LineDraw::new([36., 8.], [52., 8.], [1.; 4])
                            .width(8.)
                            .cap(LineCap::Round)
                            .antialiasing(crate::EdgeAntialiasing::None),
                    )
                    .unwrap();
                paint.draw_text(&text, TextDraw::new([0., 28.])).unwrap();
                paint
                    .draw_image(&image, TextureDraw::new(Rect::new(40., 40., 20., 8.)))
                    .unwrap();
            }
            let mut p = f
                .render_pass()
                .load_all()
                .depth_ops(None)
                .stencil_ops(None)
                .begin()
                .unwrap();
            p.set_stencil_reference(1);
            let mut paint = reader.begin(&mut p).unwrap();
            paint.fill_rect(Rect::new(0., 0., 64., 64.), BLUE).unwrap();
            let pass = paint.pass();
            assert!(matches!(
                writer
                    .shapes()
                    .draw(pass, ShapeDraw::rect(Rect::new(0., 0., 8., 8.), RED)),
                Err(Error::ReadOnlyStencil)
            ));
            assert!(matches!(
                writer
                    .lines()
                    .draw(pass, LineDraw::new([0., 0.], [8., 0.], RED)),
                Err(Error::ReadOnlyStencil)
            ));
            assert!(matches!(
                writer.text().draw(pass, &text, TextDraw::default()),
                Err(Error::ReadOnlyStencil)
            ));
            assert!(matches!(
                writer.textures().draw(pass, &image, TextureDraw::default()),
                Err(Error::ReadOnlyStencil)
            ));
        });
        assert_eq!(pixel(&bytes, 4, 4), [0; 4]);
        assert_eq!(pixel(&bytes, 12, 12), [0, 0, 255, 255]);
        assert_eq!(pixel(&bytes, 32, 4), [0; 4]);
        assert_eq!(pixel(&bytes, 40, 8), [0, 0, 255, 255]);
        assert_eq!(pixel(&bytes, 5, 38), [0, 0, 255, 255]);
        assert_eq!(pixel(&bytes, 42, 44), [0, 0, 255, 255]);
        assert_eq!(pixel(&bytes, 58, 44), [0; 4]);
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn clipped_scopes_intersect_map_through_transforms_and_restore_parent_scissor() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut t = target(&g);
        let mut painter = Painter::new(&g);
        painter.prepare(&t.render_format()).unwrap();
        let full = Rect::new(0., 0., 64., 64.);
        let bytes = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            // Application scissor set before painting bounds every clip.
            p.set_scissor_rect(0, 0, 60, 64).unwrap();
            let mut paint = painter.begin(&mut p).unwrap();
            {
                let mut scaled = paint.transformed(Transform2D::scale(2., 2.)).unwrap();
                // Local (2.25, 2, 10, 10) maps to pixels (4.5, 4, 20, 20): edges round outward.
                let mut clip = scaled.clipped(Rect::new(2.25, 2., 10., 10.)).unwrap();
                assert_eq!(clip.pass().scissor_rect(), [4, 4, 21, 20]);
                clip.fill_rect(full, RED).unwrap();
                {
                    // Nested clips intersect with their parent.
                    let mut inner = clip.clipped(Rect::new(8., 8., 20., 20.)).unwrap();
                    assert_eq!(inner.pass().scissor_rect(), [16, 16, 9, 8]);
                    inner.fill_rect(full, GREEN).unwrap();
                    // Raw changes inside a child are undone with it.
                    inner.pass().set_scissor_rect(0, 0, 1, 1).unwrap();
                }
                assert_eq!(clip.pass().scissor_rect(), [4, 4, 21, 20]);
                // Entirely outside: an empty clip that records nothing and is not an error.
                let mut outside = clip.clipped(Rect::new(100., 100., 5., 5.)).unwrap();
                assert_eq!(outside.pass().scissor_rect()[2..], [0, 0]);
                outside.fill_rect(full, BLUE).unwrap();
            }
            assert_eq!(paint.pass().scissor_rect(), [0, 0, 60, 64]);
            // Rotation clips to the transformed rectangle's bounds; beyond the
            // application scissor nothing is drawn.
            let mut rotated = paint
                .transformed(
                    Transform2D::rotation(std::f32::consts::FRAC_PI_2)
                        .then(Transform2D::translation(64., 40.)),
                )
                .unwrap();
            let mut clip = rotated.clipped(Rect::new(0., 0., 8., 20.)).unwrap();
            assert_eq!(clip.pass().scissor_rect(), [44, 40, 16, 8]);
            clip.fill_rect(Rect::new(-100., -100., 200., 200.), BLUE)
                .unwrap();
            assert!(
                clip.clipped(Rect::new(f32::NAN, 0., 1., 1.)).is_err()
                    && clip.clipped(Rect::new(0., 0., -1., 1.)).is_err()
            );
        });
        assert_eq!(pixel(&bytes, 4, 4), [255, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 24, 6), [255, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 3, 4), [0; 4]);
        assert_eq!(pixel(&bytes, 25, 10), [0; 4]);
        assert_eq!(pixel(&bytes, 4, 24), [0; 4]);
        assert_eq!(pixel(&bytes, 16, 16), [0, 255, 0, 255]);
        assert_eq!(pixel(&bytes, 24, 23), [0, 255, 0, 255]);
        assert_eq!(pixel(&bytes, 24, 24), [0; 4]);
        assert_eq!(pixel(&bytes, 50, 44), [0, 0, 255, 255]);
        assert_eq!(pixel(&bytes, 61, 44), [0; 4]);
        assert_eq!(pixel(&bytes, 50, 48), [0; 4]);
        assert!(errors.pop().await.is_none());
    });
}
#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn clipped_scopes_follow_the_viewport_origin() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let mut t = target(&g);
        let mut painter = Painter::new(&g);
        painter.prepare(&t.render_format()).unwrap();
        let bytes = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            p.set_viewport(10., 20., 40., 40., 0., 1.).unwrap();
            let mut paint = painter.begin(&mut p).unwrap();
            let mut clip = paint.clipped(Rect::new(2., 2., 4., 4.)).unwrap();
            assert_eq!(clip.pass().scissor_rect(), [12, 22, 4, 4]);
            clip.fill_rect(Rect::new(0., 0., 40., 40.), RED).unwrap();
        });
        assert_eq!(pixel(&bytes, 12, 22), [255, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 15, 25), [255, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 16, 22), [0; 4]);
        assert_eq!(pixel(&bytes, 11, 22), [0; 4]);
    });
}

// CPU oracle for a rounded rectangle: signed distance in pixels at a pixel center.
fn rounded_distance(x: usize, y: usize, rect: Rect, radius: f32) -> f32 {
    let p = [
        x as f32 + 0.5 - (rect.x + rect.width * 0.5),
        y as f32 + 0.5 - (rect.y + rect.height * 0.5),
    ];
    let q = [
        p[0].abs() - rect.width * 0.5 + radius,
        p[1].abs() - rect.height * 0.5 + radius,
    ];
    q[0].max(0.).hypot(q[1].max(0.)) + q[0].max(q[1]).min(0.) - radius
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn rounded_clips_apply_to_every_built_in_2d_renderer_with_antialiased_edges() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut t = target(&g);
        let mut painter = Painter::new(&g);
        let format = t.render_format();
        painter.prepare(&format).unwrap();
        painter.prepare_brush(&format).unwrap();
        let texture = g.create_texture(TextureOptions::new(1, 1)).unwrap();
        texture.write(&[255; 4]).unwrap();
        let image = painter
            .create_image_binding(texture.view(), TextureBindingOptions::new())
            .unwrap();
        painter.prepare_image(&image, &format).unwrap();
        let brush = g
            .create_brush(crate::BrushOptions::solid([0., 0., 1., 1.]))
            .unwrap();
        let mut builder = crate::Path::builder();
        builder
            .move_to([0., 0.])
            .line_to([64., 0.])
            .line_to([64., 64.])
            .line_to([0., 64.])
            .close();
        let square = painter
            .prepare_path(&builder.build().unwrap(), crate::PathOptions::new())
            .unwrap();
        let mut fonts = crate::TextSystem::new();
        fonts
            .load_font(include_bytes!("../tests/fonts/SourceSans3-Regular.otf"))
            .unwrap();
        let mut buffer = crate::TextBuffer::new();
        buffer
            .set_text(
                "MMMM\nMMMM",
                crate::TextStyle::new()
                    .family("Source Sans 3")
                    .font_size(40.)
                    .line_height(34.),
            )
            .unwrap();
        let text = painter
            .prepare_text(
                &buffer.layout(&mut fonts).unwrap(),
                TextRasterOptions::new(),
            )
            .unwrap();
        let full = Rect::new(0., 0., 64., 64.);
        let clip = Rect::new(8., 6., 50., 52.);
        let radius = 14.;
        type Draw<'a> = Box<dyn Fn(&mut PaintSession<'_, '_>) + 'a>;
        let draws: [(&str, Draw<'_>); 8] = [
            ("shape", Box::new(|s| s.fill_rect(full, RED).unwrap())),
            (
                "line",
                Box::new(|s| {
                    s.draw_line(LineDraw::new([0., 32.], [64., 32.], GREEN).width(64.))
                        .unwrap()
                }),
            ),
            (
                "image",
                Box::new(|s| s.draw_image(&image, TextureDraw::new(full)).unwrap()),
            ),
            (
                "path",
                Box::new(|s| s.draw_path(&square, crate::PathDraw::new(BLUE)).unwrap()),
            ),
            (
                "brush",
                Box::new(|s| {
                    s.draw_shape_with_brush(&brush, ShapeDraw::rect(full, [1.; 4]))
                        .unwrap()
                }),
            ),
            (
                "brush line",
                Box::new(|s| {
                    s.draw_line_with_brush(
                        &brush,
                        LineDraw::new([0., 32.], [64., 32.], [1.; 4]).width(64.),
                    )
                    .unwrap()
                }),
            ),
            (
                "brush path",
                Box::new(|s| {
                    s.draw_path_with_brush(&square, &brush, crate::PathDraw::new([1.; 4]))
                        .unwrap()
                }),
            ),
            (
                "text",
                Box::new(|s| s.draw_text(&text, TextDraw::new([-2., -6.])).unwrap()),
            ),
        ];
        for (name, draw) in &draws {
            let unclipped = pixels(&g, &mut t, |f| {
                let mut p = f.render_pass().begin().unwrap();
                draw(&mut painter.begin(&mut p).unwrap());
            });
            let clipped = pixels(&g, &mut t, |f| {
                let mut p = f.render_pass().begin().unwrap();
                let mut paint = painter.begin(&mut p).unwrap();
                let mut outer = paint
                    .clipped_rounded(
                        Rect::new(0., 0., 48., 64.),
                        crate::CornerRadii::uniform(22.),
                    )
                    .unwrap();
                let mut child = outer
                    .clipped_rounded(clip, crate::CornerRadii::uniform(radius))
                    .unwrap();
                draw(&mut child);
            });
            let (mut partial, mut removed) = (0, 0);
            for y in 0..64 {
                for x in 0..64 {
                    let d = rounded_distance(x, y, clip, radius).max(rounded_distance(
                        x,
                        y,
                        Rect::new(0., 0., 48., 64.),
                        22.,
                    ));
                    let (a, b) = (pixel(&unclipped, x, y), pixel(&clipped, x, y));
                    if d < -1. {
                        assert_eq!(a, b, "{name} inside at {x},{y}");
                    } else if d > 1. {
                        assert_eq!(b, [0; 4], "{name} outside at {x},{y}");
                        removed += usize::from(a[3] > 0);
                    } else if a[3] == 255 && b[3] > 0 && b[3] < 255 {
                        partial += 1;
                    }
                }
            }
            assert!(removed > 20, "{name} draws outside the clip: {removed}");
            assert!(
                *name == "text" || partial > 20,
                "{name} edges are filtered: {partial}"
            );
        }
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn rounded_clips_nest_restore_follow_rotation_and_apply_to_direct_renderers() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut t = target(&g);
        let mut painter = Painter::new(&g);
        let mut shapes = ShapeRenderer::new(&g);
        painter.prepare(&t.render_format()).unwrap();
        let full = Rect::new(0., 0., 64., 64.);
        let bytes = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            {
                let mut paint = painter.begin(&mut p).unwrap();
                {
                    let mut card = paint
                        .clipped_rounded(
                            Rect::new(0., 0., 32., 32.),
                            crate::CornerRadii::uniform(12.),
                        )
                        .unwrap();
                    {
                        // A rectangular child keeps the card's rounded corners.
                        let mut inner = card.clipped(Rect::new(0., 0., 16., 32.)).unwrap();
                        assert!(inner.pass().rounded_clip().is_some());
                        inner.fill_rect(full, RED).unwrap();
                    }
                    {
                        // Both rounded boundaries survive the nested intersection.
                        let mut inner = card
                            .clipped_rounded(Rect::new(16., 0., 32., 32.), crate::CornerRadii::ZERO)
                            .unwrap();
                        assert_eq!(inner.pass().scissor_rect(), [16, 0, 16, 32]);
                        inner.fill_rect(full, GREEN).unwrap();
                    }
                    assert_eq!(
                        card.pass().rounded_clip().unwrap().rect,
                        Rect::new(0., 0., 32., 32.)
                    );
                }
                assert!(paint.pass().rounded_clip().is_none());
                assert_eq!(paint.pass().scissor_rect(), [0, 0, 64, 64]);
                // An exact rotated clip, not just its scissor bounds.
                let mut rotated = paint
                    .transformed(
                        Transform2D::rotation(std::f32::consts::FRAC_PI_4)
                            .then(Transform2D::translation(48., 32.)),
                    )
                    .unwrap();
                let mut diamond = rotated
                    .clipped_rounded(Rect::new(-8., -8., 16., 16.), crate::CornerRadii::ZERO)
                    .unwrap();
                diamond
                    .fill_rect(Rect::new(-50., -50., 100., 100.), BLUE)
                    .unwrap();
            }
            // Direct renderers read the same pass state.
            p.set_rounded_clip(Some(crate::RoundedClip::new(
                Rect::new(0., 40., 24., 24.),
                crate::CornerRadii::uniform(12.),
            )))
            .unwrap();
            shapes.draw(&mut p, ShapeDraw::rect(full, RED)).unwrap();
            assert!(matches!(
                p.set_rounded_clip(Some(
                    crate::RoundedClip::new(full, crate::CornerRadii::ZERO)
                        .transform(Transform2D::scale(0., 1.))
                )),
                Err(Error::InvalidClip)
            ));
            assert!(p.rounded_clip().is_some());
        });
        // Both halves keep the parent card corners.
        assert_eq!(pixel(&bytes, 0, 0), [0; 4]);
        assert_eq!(pixel(&bytes, 8, 16), [255, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 31, 0), [0; 4]);
        assert_eq!(pixel(&bytes, 31, 31), [0; 4]);
        assert_eq!(pixel(&bytes, 33, 16), [0; 4]);
        // Diamond: inside its scissor bounds, but outside the rotated square.
        assert_eq!(pixel(&bytes, 48, 32), [0, 0, 255, 255]);
        assert_eq!(pixel(&bytes, 40, 25), [0; 4]);
        assert_eq!(pixel(&bytes, 55, 39), [0; 4]);
        assert_eq!(pixel(&bytes, 48, 22), [0, 0, 255, 255]);
        // Direct shape renderer: a circle.
        assert_eq!(pixel(&bytes, 12, 52), [255, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 1, 41), [0; 4]);
        assert_eq!(pixel(&bytes, 30, 52), [0; 4]);
        assert!(errors.pop().await.is_none());
    });
}

#[test]
fn layer_formats_preserve_color_precision_and_attachment_rules() {
    use wgpu::TextureFormat as F;
    for (target, layer) in [
        (F::Bgra8UnormSrgb, F::Rgba8UnormSrgb),
        (F::Rgba8UnormSrgb, F::Rgba8UnormSrgb),
        (F::Bgra8Unorm, F::Rgba8Unorm),
        (F::Rgba8Unorm, F::Rgba8Unorm),
        (F::Rgb10a2Unorm, F::Rgba16Float),
        (F::Rgba16Float, F::Rgba16Float),
        (F::R32Float, F::Rgba32Float),
        (F::Rgba32Float, F::Rgba32Float),
    ] {
        assert_eq!(RenderFormat::layer_color(target), layer);
    }
    let msaa = RenderFormat {
        colors: vec![Some(F::Bgra8Unorm)],
        depth_stencil: Some(F::Depth24PlusStencil8),
        sample_count: 4,
    };
    assert_eq!(msaa.layer().unwrap(), RenderFormat::color(F::Rgba8Unorm, 4));
    let none = RenderFormat {
        colors: vec![],
        ..msaa
    };
    assert!(matches!(none.layer(), Err(Error::ExpectedSingleColor)));
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn layer_formats_composite_nearest_sampled_layers_like_direct_painting() {
    use wgpu::TextureFormat as F;
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let scope = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut painter = Painter::new(&g);
        let shapes = [
            ShapeDraw::rect(Rect::new(4., 4., 40., 30.), [0.2, 0.5, 0.8, 0.5]),
            ShapeDraw::ellipse(Rect::new(20., 20., 36., 36.), [1., 0.8, 0., 0.75]),
        ];
        for format in [F::Rgba8UnormSrgb, F::Bgra8Unorm] {
            let mut direct = g
                .create_framebuffer(
                    FramebufferOptions::new(64, 64).format(format).usage(
                        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                    ),
                )
                .unwrap();
            let target_format = direct.render_format();
            let layer_format = target_format.layer().unwrap();
            let mut layer = g
                .create_framebuffer(
                    FramebufferOptions::new(64, 64)
                        .format(layer_format.colors[0].unwrap())
                        .usage(
                            wgpu::TextureUsages::RENDER_ATTACHMENT
                                | wgpu::TextureUsages::TEXTURE_BINDING,
                        ),
                )
                .unwrap();
            painter.prepare(&target_format).unwrap();
            painter.prepare(&layer_format).unwrap();
            let expected = pixels(&g, &mut direct, |f| {
                let mut pass = f
                    .render_pass()
                    .clear_color(wgpu::Color::BLUE)
                    .begin()
                    .unwrap();
                painter
                    .begin(&mut pass)
                    .unwrap()
                    .draw_shapes(&shapes)
                    .unwrap();
            });
            {
                let mut frame = layer.begin_frame().unwrap();
                let mut pass = frame
                    .render_pass()
                    .clear_color(wgpu::Color::TRANSPARENT)
                    .begin()
                    .unwrap();
                painter
                    .begin(&mut pass)
                    .unwrap()
                    .draw_shapes(&shapes)
                    .unwrap();
                drop(pass);
                frame.finish().unwrap();
            }
            let binding = painter
                .create_sampled_binding_with_options(
                    &layer.sampled_color(),
                    TextureBindingOptions::new()
                        .alpha(crate::TextureAlpha::Premultiplied)
                        .filter(crate::TextureFilter::Nearest),
                )
                .unwrap();
            painter.prepare_image(&binding, &target_format).unwrap();
            let composited = pixels(&g, &mut direct, |f| {
                let mut pass = f
                    .render_pass()
                    .clear_color(wgpu::Color::BLUE)
                    .begin()
                    .unwrap();
                painter
                    .begin(&mut pass)
                    .unwrap()
                    .draw_image(&binding, TextureDraw::default())
                    .unwrap();
            });
            let worst = expected
                .iter()
                .zip(&composited)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap();
            assert!(worst <= 2, "{format:?}: layer differs by {worst}");
        }
        assert!(scope.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn intersecting_rounded_clips_match_transformed_pixel_oracle_for_points_and_msaa() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut painter = Painter::new(&g);
        let mut points = g
            .create_point_buffer(crate::PointBufferOptions::new(2))
            .unwrap();
        points
            .replace(&[
                crate::Point2D::new([-100., 24.]),
                crate::Point2D::new([100., 24.]),
            ])
            .unwrap();
        let parent = Rect::new(2., 2., 45., 44.);
        let child = Rect::new(8., 4., 36., 40.);
        let transform = Transform2D::rotation(0.28).then(Transform2D::translation(9., 0.));
        let inverse = transform.invert().unwrap();
        for samples in [1, 4] {
            let mut t = g
                .create_framebuffer(
                    FramebufferOptions::new(64, 64).sample_count(samples).usage(
                        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                    ),
                )
                .unwrap();
            painter.prepare(&t.render_format()).unwrap();
            painter.prepare_points(&t.render_format()).unwrap();
            for kind in 0..3 {
                let bytes = pixels(&g, &mut t, |f| {
                    let mut pass = f.render_pass().begin().unwrap();
                    pass.set_viewport(3., 5., 60., 58., 0., 1.).unwrap();
                    let mut paint = painter.begin(&mut pass).unwrap();
                    let mut outer = paint
                        .clipped_rounded(parent, crate::CornerRadii::uniform(17.))
                        .unwrap();
                    let mut rotated = outer.transformed(transform).unwrap();
                    let mut inner = rotated
                        .clipped_rounded(child, crate::CornerRadii::uniform(13.))
                        .unwrap();
                    assert_eq!(inner.pass().rounded_clips().len(), 2);
                    // Raw raster changes do not bypass wrapped state on the next draw.
                    inner.pass().as_wgpu().set_scissor_rect(0, 0, 1, 1);
                    match kind {
                        0 => inner
                            .fill_rect(Rect::new(-100., -100., 200., 200.), RED)
                            .unwrap(),
                        1 => inner
                            .draw_polyline(
                                &points,
                                crate::PolylineDraw::new(RED).width_pixels(180.),
                            )
                            .unwrap(),
                        _ => inner
                            .draw_markers(
                                &points,
                                crate::MarkerDraw::new(RED).radius_pixels(300.).range(0..1),
                            )
                            .unwrap(),
                    }
                });
                let mut inside = 0;
                let mut removed_parent_corner = 0;
                let mut removed_child_corner = 0;
                let mut partial = 0;
                for y in 0..64 {
                    for x in 0..64 {
                        let px = x as f32 + 0.5 - 3.;
                        let py = y as f32 + 0.5 - 5.;
                        let [cx, cy] = inverse.transform_point([px, py]);
                        let distance = |x: f32, y: f32, r: Rect, radius: f32| {
                            let qx = (x - r.x - r.width * 0.5).abs() - r.width * 0.5 + radius;
                            let qy = (y - r.y - r.height * 0.5).abs() - r.height * 0.5 + radius;
                            qx.max(0.).hypot(qy.max(0.)) + qx.max(qy).min(0.) - radius
                        };
                        let a = distance(px, py, parent, 17.);
                        let b = distance(cx, cy, child, 13.);
                        let pixel = pixel(&bytes, x, y);
                        if a.max(b) < -1.5 {
                            assert_eq!(
                                pixel,
                                [255, 0, 0, 255],
                                "{samples}x kind {kind} inside {x},{y}"
                            );
                            inside += 1;
                        }
                        if a.max(b) > 1.5 {
                            assert_eq!(
                                pixel, [0; 4],
                                "{samples}x kind {kind} outside {x},{y}: {a},{b}"
                            );
                        }
                        if a > 1.5 && b < -1.5 {
                            removed_parent_corner += 1;
                        }
                        if b > 1.5 && a < -1.5 {
                            removed_child_corner += 1;
                        }
                        if pixel[3] > 0 && pixel[3] < 255 {
                            partial += 1;
                        }
                    }
                }
                assert!(
                    inside > 200
                        && removed_parent_corner > 0
                        && removed_child_corner > 0
                        && partial > 20,
                    "{samples}x kind {kind}: {inside}/{removed_parent_corner}/{removed_child_corner}/{partial}"
                );
            }
        }
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn rounded_stack_restoration_and_capacity_errors_are_atomic() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let mut t = target(&g);
        let mut painter = Painter::new(&g);
        let clip = crate::RoundedClip::new(
            Rect::new(0., 0., 32., 32.),
            crate::CornerRadii::uniform(12.),
        );
        let bytes = pixels(&g, &mut t, |f| {
            let mut pass = f.render_pass().begin().unwrap();
            pass.set_rounded_clips(&vec![clip; crate::MAX_ROUNDED_CLIPS])
                .unwrap();
            let mut paint = painter.begin(&mut pass).unwrap();
            let scissor = paint.pass().scissor_rect();
            assert!(matches!(
                paint.clipped_rounded(Rect::new(8., 8., 8., 8.), crate::CornerRadii::ZERO),
                Err(Error::ClipStackFull)
            ));
            assert_eq!(
                paint.pass().rounded_clips(),
                vec![clip; crate::MAX_ROUNDED_CLIPS]
            );
            assert_eq!(paint.pass().scissor_rect(), scissor);
            assert!(matches!(
                paint
                    .pass()
                    .set_rounded_clips(&vec![clip; crate::MAX_ROUNDED_CLIPS + 1]),
                Err(Error::ClipStackFull)
            ));
            let invalid = crate::RoundedClip::new(
                Rect::new(0., 0., 1., 1.),
                crate::CornerRadii::uniform(-1.),
            );
            assert!(matches!(
                paint.pass().set_rounded_clips(&[clip, invalid]),
                Err(Error::InvalidClip)
            ));
            {
                let mut child = paint.clipped(Rect::new(0., 0., 16., 32.)).unwrap();
                child.pass().set_rounded_clip(None).unwrap();
                child.fill_rect(Rect::new(0., 0., 64., 64.), RED).unwrap();
            }
            assert_eq!(paint.pass().rounded_clips().len(), crate::MAX_ROUNDED_CLIPS);
            paint
                .fill_rect(Rect::new(16., 0., 48., 64.), GREEN)
                .unwrap();
        });
        assert_eq!(pixel(&bytes, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 31, 0), [0; 4]);
        assert_eq!(pixel(&bytes, 20, 16), [0, 255, 0, 255]);
        assert_eq!(pixel(&bytes, 40, 16), [0; 4]);
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn custom_mesh_shader_consumes_complete_rounded_intersection() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut t = target(&g);
        let shader = g.device().create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("custom mesh with analytic intersection"),
            source: wgpu::ShaderSource::Wgsl(format!("{}\n{}", g.rounded_clip_shader(), r#"
                @vertex fn vertex_main(@location(0) position:vec3<f32>, @location(1) color:vec4<f32>) -> @builtin(position) vec4<f32> {
                    return vec4(position,1.0);
                }
                @fragment fn fragment_main(@builtin(position) position:vec4<f32>) -> @location(0) vec4<f32> {
                    let alpha = rounded_clip_coverage(position.xy);
                    return vec4(alpha,0.0,0.0,alpha);
                }
            "#).into()),
        });
        let material = g.create_material(
            crate::MaterialOptions::new(&shader)
                .bind_group_layouts(&[None, Some(&g.rounded_clip_layout())]),
        );
        let mesh = g
            .create_mesh(
                &[
                    Vertex::new([-1., -1., 0.], RED),
                    Vertex::new([3., -1., 0.], RED),
                    Vertex::new([-1., 3., 0.], RED),
                ],
                &[0, 1, 2],
            )
            .unwrap();
        let mut meshes = MeshRenderer::new(&g);
        let mut painter = Painter::new(&g);
        let clips = [
            crate::RoundedClip::new(
                Rect::new(0., 0., 48., 48.),
                crate::CornerRadii::uniform(20.),
            ),
            crate::RoundedClip::new(
                Rect::new(16., 0., 48., 48.),
                crate::CornerRadii::uniform(20.),
            ),
        ];
        let expected = pixels(&g, &mut t, |f| {
            let mut pass = f.render_pass().begin().unwrap();
            pass.set_rounded_clips(&clips).unwrap();
            painter
                .begin(&mut pass)
                .unwrap()
                .fill_rect(Rect::new(0., 0., 64., 64.), RED)
                .unwrap();
        });
        let actual = pixels(&g, &mut t, |f| {
            let mut pass = f.render_pass().begin().unwrap();
            pass.set_rounded_clips(&clips).unwrap();
            pass.bind_rounded_clips();
            meshes
                .draw_with_material(&mut pass, &mesh, &material)
                .unwrap();
            // Clear and rebind after another renderer used its own bindings.
            pass.set_rounded_clip(None).unwrap();
            painter
                .begin(&mut pass)
                .unwrap()
                .fill_rect(Rect::new(60., 60., 4., 4.), RED)
                .unwrap();
            pass.set_rounded_clips(&clips).unwrap();
            pass.bind_rounded_clips();
            pass.as_wgpu().set_scissor_rect(0, 0, 64, 64); // raw access remains available
        });
        for y in 0..60 {
            for x in 0..64 {
                assert_eq!(pixel(&expected, x, y), pixel(&actual, x, y), "{x},{y}");
            }
        }
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn rounded_clip_binding_tracks_viewport_changes_and_raw_group_overrides() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut t = target(&g);
        let mut painter = Painter::new(&g);
        let clip = crate::RoundedClip::new(
            Rect::new(0., 0., 24., 24.),
            crate::CornerRadii::uniform(12.),
        );
        let disabled = g.device().create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: crate::clip::stack_size(g.rounded_clip_capacity()),
            usage: wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: false,
        });
        let disabled_group = g.clip_group(Some((&disabled, 0)));
        let bytes = pixels(&g, &mut t, |f| {
            let mut pass = f.render_pass().begin().unwrap();
            pass.set_rounded_clips(&[clip, clip]).unwrap();
            for (x, y, w, h, color) in [
                (0., 0., 24., 24., RED),
                (32., 0., 24., 24., GREEN),
                (0., 32., 32., 24., BLUE),
            ] {
                pass.set_viewport(x, y, w, h, 0., 1.).unwrap();
                // A raw group-one replacement must not fool the wrapped binding cache.
                pass.as_wgpu().set_bind_group(1, &disabled_group, &[]);
                painter
                    .begin(&mut pass)
                    .unwrap()
                    .fill_rect(Rect::new(0., 0., 64., 64.), color)
                    .unwrap();
            }
        });
        for (x, y, color) in [
            (0, 0, [255, 0, 0, 255]),
            (32, 0, [0, 255, 0, 255]),
            (0, 32, [0, 0, 255, 255]),
        ] {
            assert_eq!(pixel(&bytes, x + 12, y + 12), color);
            assert_eq!(pixel(&bytes, x, y), [0; 4]);
            assert_eq!(pixel(&bytes, x + 23, y + 23), [0; 4]);
        }
        // Repeating a boundary up to the checked limit preserves fractional edges.
        let mut images = Vec::new();
        for count in [1, crate::MAX_ROUNDED_CLIPS] {
            images.push(pixels(&g, &mut t, |f| {
                let mut pass = f.render_pass().begin().unwrap();
                pass.set_rounded_clips(&vec![clip; count]).unwrap();
                painter
                    .begin(&mut pass)
                    .unwrap()
                    .fill_rect(Rect::new(0., 0., 64., 64.), RED)
                    .unwrap();
            }));
        }
        assert_eq!(images[0], images[1]);
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn rounded_clip_capacity_respects_application_uniform_limits() {
    pollster::block_on(async {
        let base = GraphicsContext::headless().await.unwrap();
        for uniforms in [0, 1, 2] {
            let (device, queue) = base
                .adapter()
                .request_device(&wgpu::DeviceDescriptor {
                    required_limits: wgpu::Limits {
                        max_buffer_size: 256,
                        max_uniform_buffer_binding_size: 128,
                        max_uniform_buffers_per_shader_stage: uniforms,
                        max_storage_buffer_binding_size: 128,
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .await
                .unwrap();
            let g = GraphicsContext::from_wgpu(
                base.instance().clone(),
                base.adapter().clone(),
                device,
                queue,
            );
            let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
            let capacity = if uniforms < 2 { 0 } else { 2 };
            assert_eq!(g.rounded_clip_capacity(), capacity);
            assert_eq!(g.clone().rounded_clip_capacity(), capacity);
            assert_eq!(base.rounded_clip_capacity(), crate::MAX_ROUNDED_CLIPS);
            let mut t = g
                .create_framebuffer(
                    FramebufferOptions::new(8, 1).usage(
                        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                    ),
                )
                .unwrap();
            let mut painter = Painter::new(&g);
            let clip = crate::RoundedClip::new(
                Rect::new(2., -4., 8., 8.),
                crate::CornerRadii::uniform(12.),
            );
            let bytes = pixels(&g, &mut t, |f| {
                let mut pass = f.render_pass().begin().unwrap();
                painter
                    .begin(&mut pass)
                    .unwrap()
                    .draw_shape(
                        ShapeDraw::rect(Rect::new(7., 0., 1., 1.), BLUE)
                            .antialiasing(crate::EdgeAntialiasing::None),
                    )
                    .unwrap();
                assert!(matches!(
                    pass.set_rounded_clips(&vec![clip; capacity + 1]),
                    Err(Error::ClipStackFull)
                ));
                pass.set_rounded_clips(&vec![clip; capacity]).unwrap();
                painter
                    .begin(&mut pass)
                    .unwrap()
                    .draw_shape(
                        ShapeDraw::rect(Rect::new(0., 0., 6., 1.), RED)
                            .antialiasing(crate::EdgeAntialiasing::None),
                    )
                    .unwrap();
            });
            assert_eq!(pixel(&bytes, 7, 0), [0, 0, 255, 255]);
            assert_eq!(pixel(&bytes, 4, 0), [255, 0, 0, 255]);
            assert_eq!(
                pixel(&bytes, 0, 0),
                if capacity == 0 {
                    [255, 0, 0, 255]
                } else {
                    [0; 4]
                }
            );
            assert!(errors.pop().await.is_none());
        }
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn warm_clip_bindings_preserve_pixels_and_unsubmitted_recording_isolation() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut a = target(&g);
        let mut b = target(&g);
        let mut painter = Painter::new(&g);
        let clips = [
            crate::RoundedClip::new(
                Rect::new(4., 4., 48., 48.),
                crate::CornerRadii::uniform(12.),
            ),
            crate::RoundedClip::new(
                Rect::new(20., 0., 40., 60.),
                crate::CornerRadii::uniform(10.),
            ),
        ];
        let mut record = |pass: &mut RenderPass<'_>, shifted: bool| {
            pass.set_rounded_clips(&clips).unwrap();
            if shifted {
                pass.set_viewport(8., 0., 56., 64., 0., 1.).unwrap();
            }
            painter
                .begin(pass)
                .unwrap()
                .fill_rect(Rect::new(0., 0., 64., 64.), RED)
                .unwrap();
        };
        let expected = pixels(&g, &mut a, |frame| {
            record(&mut frame.render_pass().begin().unwrap(), false)
        });
        assert_eq!(pixel(&expected, 28, 28), [255, 0, 0, 255]);
        assert_eq!(pixel(&expected, 10, 28), [0; 4]);
        let shifted = pixels(&g, &mut a, |frame| {
            record(&mut frame.render_pass().begin().unwrap(), true)
        });
        assert_ne!(expected, shifted);
        // Submit B before A, while A still leases its original upload page.
        let actual = pixels(&g, &mut a, |frame| {
            record(&mut frame.render_pass().begin().unwrap(), false);
            let mut other = b.begin_frame().unwrap();
            record(&mut other.render_pass().begin().unwrap(), true);
            other.finish().unwrap();
        });
        assert_eq!(actual, expected);
        let before = g.upload_stats();
        for shifted_frame in [false, true, false, true] {
            let actual = pixels(&g, &mut a, |frame| {
                record(&mut frame.render_pass().begin().unwrap(), shifted_frame)
            });
            assert_eq!(
                actual,
                if shifted_frame { &shifted } else { &expected }.clone()
            );
        }
        let after = g.upload_stats();
        assert_eq!(before.pages_created, after.pages_created);
        assert_eq!(before.clip_bindings_created, after.clip_bindings_created);
        assert!(after.pages_reused > before.pages_reused);
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn imported_restricted_devices_preserve_unclipped_draws_and_custom_clip_api() {
    pollster::block_on(async {
        let base = GraphicsContext::headless().await.unwrap();
        // Exercise independent group/uniform restrictions, including devices where
        // brushes have exactly their original one-uniform budget.
        for (groups, uniforms) in [(1, 0), (1, 1), (1, 2), (4, 0), (4, 1)] {
            let (device, queue) = base
                .adapter()
                .request_device(&wgpu::DeviceDescriptor {
                    required_limits: wgpu::Limits {
                        max_bind_groups: groups,
                        max_uniform_buffers_per_shader_stage: uniforms,
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .await
                .unwrap();
            let g = GraphicsContext::from_wgpu(
                base.instance().clone(),
                base.adapter().clone(),
                device,
                queue,
            );
            let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
            assert_eq!(g.rounded_clip_capacity(), 0);
            let mut t = target(&g);
            let mut painter = Painter::new(&g);
            let format = t.render_format();
            painter.prepare(&format).unwrap();
            if uniforms > 0 {
                painter.prepare_brush(&format).unwrap();
            }
            painter.prepare_points(&format).unwrap();
            let texture = g.create_texture(TextureOptions::new(1, 1)).unwrap();
            texture.write(&[255; 4]).unwrap();
            let image = painter
                .create_image_binding(texture.view(), TextureBindingOptions::new())
                .unwrap();
            painter.prepare_image(&image, &format).unwrap();
            let brush = (uniforms > 0).then(|| {
                g.create_brush(crate::BrushOptions::solid([0., 0., 1., 1.]))
                    .unwrap()
            });
            let mut points = g
                .create_point_buffer(crate::PointBufferOptions::new(2))
                .unwrap();
            points
                .replace(&[
                    crate::Point2D::new([0., 32.]),
                    crate::Point2D::new([64., 32.]),
                ])
                .unwrap();
            let mut builder = crate::Path::builder();
            builder
                .move_to([0., 0.])
                .line_to([64., 0.])
                .line_to([64., 64.])
                .line_to([0., 64.])
                .close();
            let square = painter
                .prepare_path(&builder.build().unwrap(), crate::PathOptions::new())
                .unwrap();
            let mut fonts = crate::TextSystem::new();
            fonts
                .load_font(include_bytes!("../tests/fonts/SourceSans3-Regular.otf"))
                .unwrap();
            let mut buffer = crate::TextBuffer::new();
            buffer
                .set_text(
                    "MMMM\nMMMM",
                    crate::TextStyle::new()
                        .family("Source Sans 3")
                        .font_size(40.)
                        .line_height(34.),
                )
                .unwrap();
            let text = painter
                .prepare_text(
                    &buffer.layout(&mut fonts).unwrap(),
                    TextRasterOptions::new(),
                )
                .unwrap();
            let full = Rect::new(0., 0., 64., 64.);
            type Draw<'a> = Box<dyn Fn(&mut PaintSession<'_, '_>) + 'a>;
            let draws: [(&str, Draw<'_>); 10] = [
                ("shape", Box::new(|s| s.fill_rect(full, RED).unwrap())),
                (
                    "line",
                    Box::new(|s| {
                        s.draw_line(LineDraw::new([0., 32.], [64., 32.], GREEN).width(64.))
                            .unwrap()
                    }),
                ),
                (
                    "image",
                    Box::new(|s| s.draw_image(&image, TextureDraw::new(full)).unwrap()),
                ),
                (
                    "path",
                    Box::new(|s| s.draw_path(&square, crate::PathDraw::new(BLUE)).unwrap()),
                ),
                (
                    "brush",
                    Box::new(|s| {
                        s.draw_shape_with_brush(
                            brush.as_ref().unwrap(),
                            ShapeDraw::rect(full, [1.; 4]),
                        )
                        .unwrap()
                    }),
                ),
                (
                    "brush line",
                    Box::new(|s| {
                        s.draw_line_with_brush(
                            brush.as_ref().unwrap(),
                            LineDraw::new([0., 32.], [64., 32.], [1.; 4]).width(64.),
                        )
                        .unwrap()
                    }),
                ),
                (
                    "brush path",
                    Box::new(|s| {
                        s.draw_path_with_brush(
                            &square,
                            brush.as_ref().unwrap(),
                            crate::PathDraw::new([1.; 4]),
                        )
                        .unwrap()
                    }),
                ),
                (
                    "polyline",
                    Box::new(|s| {
                        s.draw_polyline(&points, crate::PolylineDraw::new(RED).width_pixels(32.))
                            .unwrap()
                    }),
                ),
                (
                    "markers",
                    Box::new(|s| {
                        s.draw_markers(&points, crate::MarkerDraw::new(RED).radius_pixels(48.))
                            .unwrap()
                    }),
                ),
                (
                    "text",
                    Box::new(|s| s.draw_text(&text, TextDraw::new([-2., -6.])).unwrap()),
                ),
            ];

            for (name, draw) in &draws {
                if uniforms == 0 && name.starts_with("brush") {
                    continue;
                }
                let bytes = pixels(&g, &mut t, |f| {
                    let mut pass = f.render_pass().begin().unwrap();
                    pass.set_scissor_rect(0, 0, 64, 64).unwrap();
                    let clip = crate::RoundedClip::new(
                        Rect::new(0., 0., 1., 1.),
                        crate::CornerRadii::uniform(0.),
                    );
                    assert!(matches!(
                        pass.set_rounded_clip(Some(clip)),
                        Err(Error::ClipStackFull)
                    ));
                    assert!(matches!(
                        pass.set_rounded_clips(&[clip]),
                        Err(Error::ClipStackFull)
                    ));
                    assert!(pass.rounded_clips().is_empty());
                    pass.set_rounded_clips(&[]).unwrap();
                    // Also exercise the public custom binding API on a device
                    // where set_bind_group(1) itself would be invalid.
                    pass.bind_rounded_clips();
                    let mut paint = painter.begin(&mut pass).unwrap();
                    assert!(matches!(
                        paint.clipped_rounded(
                            Rect::new(0., 0., 1., 1.),
                            crate::CornerRadii::uniform(0.)
                        ),
                        Err(Error::ClipStackFull)
                    ));
                    assert!(paint.pass().rounded_clips().is_empty());
                    draw(&mut paint);
                });
                assert!(
                    bytes.as_chunks::<4>().0.iter().filter(|p| p[3] > 0).count() > 100,
                    "groups={groups} uniforms={uniforms} renderer={name}: empty or unexpectedly clipped"
                );
                if *name != "text" {
                    assert_eq!(pixel(&bytes, 32, 32)[3], 255, "{name}");
                }
            }
            // Public custom shader integration follows the documented capacity
            // branch: omit group one entirely when no analytic clips are available.
            let clip_layout = g.rounded_clip_layout();
            let layouts = if g.rounded_clip_capacity() > 0 {
                vec![None, Some(&clip_layout)]
            } else {
                vec![]
            };
            let shader = g.device().create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("custom clip fallback"),
                source: wgpu::ShaderSource::Wgsl(format!("{}\n{}", g.rounded_clip_shader(), r#"
                    @vertex fn vs(@location(0) p:vec3<f32>) -> @builtin(position) vec4<f32> { return vec4(p,1.0); }
                    @fragment fn fs(@builtin(position) p:vec4<f32>) -> @location(0) vec4<f32> { return vec4(0.0, rounded_clip_coverage(p.xy), 0.0, 1.0); }
                "#).into()),
            });
            let material = g.create_material(
                crate::MaterialOptions::new(&shader)
                    .entry_points("vs", "fs")
                    .bind_group_layouts(&layouts),
            );
            let mesh = g
                .create_mesh(
                    &[
                        Vertex::new([-1., -1., 0.], RED),
                        Vertex::new([3., -1., 0.], RED),
                        Vertex::new([-1., 3., 0.], RED),
                    ],
                    &[0, 1, 2],
                )
                .unwrap();
            let mut meshes = MeshRenderer::new(&g);
            let bytes = pixels(&g, &mut t, |f| {
                let mut pass = f.render_pass().begin().unwrap();
                pass.bind_rounded_clips();
                meshes
                    .draw_with_material(&mut pass, &mesh, &material)
                    .unwrap();
            });
            assert_eq!(pixel(&bytes, 32, 32), [0, 255, 0, 255]);
            assert!(
                errors.pop().await.is_none(),
                "groups={groups} uniforms={uniforms}"
            );
        }
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn repeated_rounded_bindings_restore_after_application_dynamic_offsets() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut t = target(&g);
        let mut painter = Painter::new(&g);
        let stride = g.device().limits().min_uniform_buffer_offset_alignment as u64;
        let buffer = g.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("application dynamic uniform"),
            size: stride + 16,
            usage: wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: false,
        });
        let layout = g
            .device()
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: std::num::NonZeroU64::new(16),
                    },
                    count: None,
                }],
            });
        let group = g.device().create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: std::num::NonZeroU64::new(16),
                }),
            }],
        });
        let render = |pass: &mut RenderPass<'_>, painter: &mut Painter, override_groups: bool| {
            // Reuse each disabled/active binding repeatedly, then change the full
            // stack and return to disabled. Application group one must be restored
            // even when its layout or dynamic offsets differ from the clip layout.
            for clips in [0, 2, 1, 0] {
                let clip = crate::RoundedClip::new(
                    Rect::new(4., 4., 56., 56.),
                    crate::CornerRadii::uniform(16.),
                );
                pass.set_rounded_clips(&vec![clip; clips]).unwrap();
                for i in 0..3 {
                    pass.bind_rounded_clips();
                    pass.bind_rounded_clips();
                    if override_groups {
                        pass.set_bind_group(
                            1,
                            &group,
                            &[if i % 2 == 0 { 0 } else { stride as u32 }],
                        );
                    }
                    painter
                        .begin(pass)
                        .unwrap()
                        .fill_rect(Rect::new((clips * 16 + i * 4) as f32, 0., 4., 64.), RED)
                        .unwrap();
                }
            }
        };
        let expected = pixels(&g, &mut t, |f| {
            render(&mut f.render_pass().begin().unwrap(), &mut painter, false);
        });
        let actual = pixels(&g, &mut t, |f| {
            render(&mut f.render_pass().begin().unwrap(), &mut painter, true);
        });
        assert_eq!(actual, expected);
        assert_eq!(pixel(&actual, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel(&actual, 32, 0), [0; 4]);
        assert_eq!(pixel(&actual, 32, 32), [255, 0, 0, 255]);
        assert!(errors.pop().await.is_none());
    });
}
