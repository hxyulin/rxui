use super::*;
use crate::framebuffer::tests::pixels;
use crate::{
    Framebuffer, FramebufferOptions, MeshRenderer, Stroke, TextureBindingOptions, TextureDraw,
    TextureOptions, TextureRenderer, Vertex,
};

fn target(g: &GraphicsContext, samples: u32, depth: bool) -> Framebuffer {
    let mut options = FramebufferOptions::new(64, 64).sample_count(samples).usage(
        wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    if depth {
        options = options.depth_stencil(wgpu::TextureFormat::Depth24PlusStencil8);
    }
    g.create_framebuffer(options).unwrap()
}
fn pixel(p: &[u8], x: usize, y: usize) -> [u8; 4] {
    p[(y * 64 + x) * 4..(y * 64 + x + 1) * 4]
        .try_into()
        .unwrap()
}
fn near(p: [u8; 4], expected: [u8; 4]) {
    for (a, b) in p.into_iter().zip(expected) {
        assert!(a.abs_diff(b) <= 2, "{p:?} != {expected:?}");
    }
}
const RED: [f32; 4] = [1., 0., 0., 1.];
const GREEN: [f32; 4] = [0., 1., 0., 1.];
const BLUE: [f32; 4] = [0., 0., 1., 1.];

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn shapes_and_line_caps_have_expected_coverage() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut t = target(&g, 1, false);
        let mut shapes = ShapeRenderer::new(&g);
        let mut lines = LineRenderer::new(&g);
        shapes.prepare(&t.render_format()).unwrap();
        lines.prepare(&t.render_format()).unwrap();
        let bytes = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            shapes
                .draw_many(
                    &mut p,
                    &[
                        ShapeDraw::rect(Rect::new(2., 2., 12., 12.), RED),
                        ShapeDraw::rounded_rect(Rect::new(20., 2., 16., 16.), 8., GREEN),
                        ShapeDraw::ellipse(Rect::new(42., 2., 18., 12.), BLUE),
                    ],
                )
                .unwrap();
            for (y, cap) in [
                (28., LineCap::Butt),
                (40., LineCap::Square),
                (52., LineCap::Round),
            ] {
                lines
                    .draw(
                        &mut p,
                        LineDraw::new([10., y], [24., y], RED).width(8.).cap(cap),
                    )
                    .unwrap();
            }
            lines
                .draw_many(
                    &mut p,
                    &[
                        LineDraw::new([44., 28.], [44., 28.], GREEN)
                            .width(8.)
                            .cap(LineCap::Round),
                        LineDraw::new([44., 42.], [44., 42.], BLUE)
                            .width(8.)
                            .cap(LineCap::Square),
                        LineDraw::new([44., 54.], [44., 54.], RED).width(8.),
                    ],
                )
                .unwrap();
        });
        assert_eq!(pixel(&bytes, 8, 8), [255, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 28, 10), [0, 255, 0, 255]);
        assert_eq!(pixel(&bytes, 20, 2), [0; 4]);
        assert_eq!(pixel(&bytes, 51, 8), [0, 0, 255, 255]);
        assert_eq!(pixel(&bytes, 42, 2), [0; 4]);
        assert_eq!(pixel(&bytes, 7, 28), [0; 4]);
        assert_eq!(pixel(&bytes, 7, 40), [255, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 7, 52), [255, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 6, 48), [0; 4]);
        assert_eq!(pixel(&bytes, 44, 28), [0, 255, 0, 255]);
        assert_eq!(pixel(&bytes, 41, 39), [0, 0, 255, 255]);
        assert_eq!(pixel(&bytes, 44, 54), [0; 4]);
        assert!(errors.pop().await.is_none());
    });
}
#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn batches_match_individual_draws_and_preserve_translucent_order() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let mut t = target(&g, 1, false);
        let mut shapes = ShapeRenderer::new(&g);
        let mut lines = LineRenderer::new(&g);
        let draws = [
            ShapeDraw::rect(Rect::new(4., 4., 40., 40.), [1., 0., 0., 0.5]),
            ShapeDraw::ellipse(Rect::new(12., 12., 40., 40.), [0., 0., 1., 0.5]),
            ShapeDraw::rounded_rect(Rect::new(20., 20., 32., 32.), 8., [0., 1., 0., 0.5]),
        ];
        let segments = [
            LineDraw::new([4., 58.], [58., 4.], RED)
                .width(3.)
                .cap(LineCap::Round),
            LineDraw::new([4., 4.], [58., 58.], [0., 1., 0., 0.5])
                .width(4.)
                .cap(LineCap::Square),
        ];
        let individual = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            for d in draws {
                shapes.draw(&mut p, d).unwrap();
            }
            for d in segments {
                lines.draw(&mut p, d).unwrap();
            }
        });
        let batched = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            shapes.draw_many(&mut p, &draws).unwrap();
            lines.draw_many(&mut p, &segments).unwrap();
        });
        assert_eq!(individual, batched);
        near(pixel(&batched, 32, 25), [32, 128, 64, 223]);
        // Batch crosses three page boundaries; the last overlapping draw must win.
        let mut many = vec![ShapeDraw::rect(Rect::new(0., 0., 64., 64.), RED); 2050];
        many[2049].color = BLUE;
        let bytes = pixels(&g, &mut t, |f| {
            shapes
                .draw_many(&mut f.render_pass().begin().unwrap(), &many)
                .unwrap();
        });
        assert_eq!(pixel(&bytes, 32, 32), [0, 0, 255, 255]);
    });
}
#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn viewport_transforms_clipping_and_fractional_edges_are_consistent() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let mut t = target(&g, 1, false);
        let mut shapes = ShapeRenderer::new(&g);
        let bytes = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            p.set_viewport(16., 8., 32., 40., 0., 1.).unwrap();
            p.set_scissor_rect(16, 8, 32, 40).unwrap();
            shapes
                .draw(
                    &mut p,
                    ShapeDraw::rect(Rect::new(0., 0., 0.5, 0.5), RED).space(DrawSpace::Normalized),
                )
                .unwrap();
            shapes
                .draw(
                    &mut p,
                    ShapeDraw::rect(Rect::new(0., 0., 8., 8.), GREEN).transform(
                        Transform2D::rotation(std::f32::consts::FRAC_PI_2)
                            .then(Transform2D::translation(24., 24.)),
                    ),
                )
                .unwrap();
            p.set_viewport(0., 0., 64., 64., 0., 1.).unwrap();
            p.set_scissor_rect(0, 0, 64, 64).unwrap();
            shapes
                .draw(&mut p, ShapeDraw::rect(Rect::new(4.5, 52., 8., 8.), BLUE))
                .unwrap();
            shapes
                .draw(
                    &mut p,
                    ShapeDraw::rect(Rect::new(24.5, 52., 8., 8.), RED)
                        .antialiasing(EdgeAntialiasing::None),
                )
                .unwrap();
            shapes
                .draw(
                    &mut p,
                    ShapeDraw::rect(Rect::new(0., 0., 64., 64.), BLUE)
                        .transform(Transform2D::scale(0., 1.)),
                )
                .unwrap();
        });
        assert_eq!(pixel(&bytes, 18, 10), [255, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 10, 10), [0; 4]);
        assert_eq!(pixel(&bytes, 34, 36), [0, 255, 0, 255]);
        near(pixel(&bytes, 4, 56), [0, 0, 128, 128]);
        assert_eq!(pixel(&bytes, 3, 56), [0; 4]);
        assert_eq!(pixel(&bytes, 24, 56), [255, 0, 0, 255]);
    });
}
#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn scoped_restoration_shares_mesh_texture_msaa_and_readonly_depth() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut t = target(&g, 4, true);
        let mut shapes = ShapeRenderer::new(&g);
        let mut lines = LineRenderer::new(&g);
        let mut meshes = MeshRenderer::new(&g);
        let mut textures = TextureRenderer::new(&g);
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
        let image = g.create_texture(TextureOptions::new(1, 1)).unwrap();
        image.write(&[255; 4]).unwrap();
        let binding = textures
            .create_binding(image.view(), TextureBindingOptions::new())
            .unwrap();
        let bytes = pixels(&g, &mut t, |f| {
            // Initialize depth/stencil before taking both aspects read-only.
            drop(f.render_pass().begin().unwrap());
            let mut p = f
                .render_pass()
                .load_all()
                .depth_ops(None)
                .stencil_ops(None)
                .begin()
                .unwrap();
            let mut s = shapes.bind(&mut p).unwrap();
            s.draw(ShapeDraw::rect(Rect::new(0., 0., 64., 64.), RED))
                .unwrap();
            meshes.draw(s.pass(), &mesh).unwrap();
            textures
                .draw(
                    s.pass(),
                    &binding,
                    TextureDraw::new(Rect::new(0., 0., 8., 8.)),
                )
                .unwrap();
            lines
                .draw(
                    s.pass(),
                    LineDraw::new([8., 56.], [56., 56.], GREEN).width(4.),
                )
                .unwrap();
            s.pass().as_wgpu().set_scissor_rect(0, 0, 1, 1);
            s.draw(ShapeDraw::ellipse(Rect::new(20., 20., 24., 24.), GREEN))
                .unwrap();
            drop(s);
            let mut l = lines.bind(&mut p).unwrap();
            l.draw(LineDraw::new([4., 12.], [60., 12.], BLUE).width(4.))
                .unwrap();
            shapes
                .draw(l.pass(), ShapeDraw::rect(Rect::new(48., 16., 8., 8.), RED))
                .unwrap();
            shapes
                .draw(l.pass(), ShapeDraw::rect(Rect::new(0., 40., 64., 8.), RED))
                .unwrap();
            l.pass().set_scissor_rect(0, 0, 32, 64).unwrap();
            l.draw(LineDraw::new([4., 44.], [60., 44.], BLUE).width(4.))
                .unwrap();
        });
        assert_eq!(pixel(&bytes, 32, 32), [0, 255, 0, 255]);
        assert_eq!(pixel(&bytes, 4, 4), [255; 4]);
        assert_eq!(pixel(&bytes, 20, 12), [0, 0, 255, 255]);
        assert_eq!(pixel(&bytes, 20, 44), [0, 0, 255, 255]);
        assert_eq!(pixel(&bytes, 50, 44), [255, 0, 0, 255]);
        assert!(errors.pop().await.is_none());
    });
}
#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn invalid_batches_are_atomic_and_warm_resources_are_reused() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let mut t = target(&g, 1, false);
        let mut shapes = ShapeRenderer::new(&g);
        let mut lines = LineRenderer::new(&g);
        shapes.prepare(&t.render_format()).unwrap();
        lines.prepare(&t.render_format()).unwrap();
        let batch = vec![ShapeDraw::rect(Rect::new(0., 0., 64., 64.), RED); 10];
        let render = |f: &mut crate::Frame<'_, '_>, s: &mut ShapeRenderer, l: &mut LineRenderer| {
            let mut p = f
                .render_pass()
                .clear_color(wgpu::Color::BLUE)
                .begin()
                .unwrap();
            assert!(matches!(
                s.draw_many(
                    &mut p,
                    &[
                        batch[0],
                        ShapeDraw::ellipse(Rect::new(0., 0., -1., 1.), RED)
                    ]
                ),
                Err(Error::InvalidShapeDraw)
            ));
            assert!(matches!(
                l.draw_many(
                    &mut p,
                    &[
                        LineDraw::new([0., 32.], [64., 32.], RED).width(64.),
                        LineDraw::new([0.; 2], [1.; 2], RED).width(-1.)
                    ]
                ),
                Err(Error::InvalidLineDraw)
            ));
            // Overflow is rejected before drawing, even though the supplied values are finite.
            assert!(matches!(
                s.draw(
                    &mut p,
                    ShapeDraw::rect(Rect::new(f32::MAX, 0., f32::MAX, 8.), RED)
                ),
                Err(Error::InvalidShapeDraw)
            ));
            assert!(matches!(
                l.draw(&mut p, LineDraw::new([-f32::MAX, 0.], [f32::MAX, 0.], RED)),
                Err(Error::InvalidLineDraw)
            ));
            s.draw_many(&mut p, &batch).unwrap();
        };
        pixels(&g, &mut t, |f| render(f, &mut shapes, &mut lines));
        let buffer = g.upload_pool.lock().unwrap()[0].0.clone();
        let capacity = shapes.inner.parameters.capacity();
        let pipeline = shapes.inner.pipelines.values().next().unwrap().clone();
        pixels(&g, &mut t, |f| render(f, &mut shapes, &mut lines));
        assert_eq!(g.upload_pool.lock().unwrap()[0].0, buffer);
        assert_eq!(shapes.inner.parameters.capacity(), capacity);
        assert_eq!(shapes.inner.pipelines.values().next().unwrap(), &pipeline);
        let bytes = pixels(&g, &mut t, |f| {
            let mut p = f
                .render_pass()
                .clear_color(wgpu::Color::BLUE)
                .begin()
                .unwrap();
            assert!(
                shapes
                    .draw_many(
                        &mut p,
                        &[
                            batch[0],
                            ShapeDraw::rect(Rect::new(0., 0., 64., 64.), [0., 0., 0., 2.])
                        ]
                    )
                    .is_err()
            );
            assert!(
                lines
                    .draw_many(
                        &mut p,
                        &[
                            LineDraw::new([0., 32.], [64., 32.], RED).width(64.),
                            LineDraw::new([f32::NAN, 0.], [0.; 2], RED)
                        ]
                    )
                    .is_err()
            );
        });
        assert_eq!(pixel(&bytes, 32, 32), [0, 0, 255, 255]);
        let foreign = GraphicsContext::headless().await.unwrap();
        let mut foreign_shapes = ShapeRenderer::new(&foreign);
        let mut f = t.begin_frame().unwrap();
        let mut p = f.render_pass().begin().unwrap();
        assert!(matches!(
            foreign_shapes.bind(&mut p),
            Err(Error::DeviceMismatch)
        ));
    });
}
#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn mixed_instance_strides() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let mut t = target(&g, 1, false);
        let mut s = ShapeRenderer::new(&g);
        let mut r = TextureRenderer::new(&g);
        let i = g.create_texture(TextureOptions::new(1, 1)).unwrap();
        i.write(&[255; 4]).unwrap();
        let b = r
            .create_binding(i.view(), TextureBindingOptions::new())
            .unwrap();
        let bytes = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            s.draw(&mut p, ShapeDraw::rect(Rect::new(0., 0., 64., 64.), RED))
                .unwrap();
            r.draw(&mut p, &b, TextureDraw::new(Rect::new(0., 0., 8., 8.)))
                .unwrap();
        });
        assert_eq!(pixel(&bytes, 4, 4), [255; 4]);
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn subpixel_geometry_does_not_become_a_half_opaque_pixel() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let mut t = target(&g, 1, false);
        let mut lines = LineRenderer::new(&g);
        let mut shapes = ShapeRenderer::new(&g);
        let bytes = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            lines
                .draw(
                    &mut p,
                    LineDraw::new([2., 8.5], [14., 8.5], RED).width(0.25),
                )
                .unwrap();
            shapes
                .draw(
                    &mut p,
                    ShapeDraw::rect(Rect::new(20.375, 16., 0.25, 8.), GREEN),
                )
                .unwrap();
            lines
                .draw(
                    &mut p,
                    LineDraw::new([2., 32.5], [14., 32.5], BLUE).width(f32::from_bits(1)),
                )
                .unwrap();
        });
        near(pixel(&bytes, 8, 8), [64, 0, 0, 64]);
        near(pixel(&bytes, 20, 20), [0, 64, 0, 64]);
        assert_eq!(pixel(&bytes, 8, 32), [0; 4]);
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn outlines_place_width_and_preserve_translucent_corners() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut t = target(&g, 1, false);
        let mut shapes = ShapeRenderer::new(&g);
        let bytes = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            shapes
                .draw_many(
                    &mut p,
                    &[
                        ShapeDraw::rect(Rect::new(6., 6., 12., 12.), RED)
                            .stroke(Stroke::new(2.).inside()),
                        ShapeDraw::rect(Rect::new(26., 6., 12., 12.), GREEN)
                            .stroke(Stroke::new(2.)),
                        ShapeDraw::rect(Rect::new(46., 6., 12., 12.), BLUE)
                            .stroke(Stroke::new(2.).outside()),
                        ShapeDraw::rect(Rect::new(6., 26., 12., 12.), [1., 0., 0., 0.5])
                            .stroke(Stroke::new(3.).inside()),
                        ShapeDraw::rounded_rect(Rect::new(26., 26., 16., 16.), 6., GREEN)
                            .stroke(Stroke::new(3.).inside()),
                        ShapeDraw::rect(Rect::new(48., 28., 8., 8.), BLUE)
                            .stroke(Stroke::new(20.).inside()),
                        ShapeDraw::rect(Rect::new(4., 50.5, 12., 8.), RED)
                            .stroke(Stroke::new(0.25).inside()),
                        ShapeDraw::rect(Rect::new(24., 50.5, 12., 8.), GREEN)
                            .stroke(Stroke::new(0.25)),
                        ShapeDraw::rect(Rect::new(44., 50.5, 12., 8.), BLUE)
                            .stroke(Stroke::new(0.25).outside()),
                    ],
                )
                .unwrap();
        });
        assert_eq!(pixel(&bytes, 5, 12), [0; 4]);
        assert_eq!(pixel(&bytes, 6, 12), [255, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 8, 12), [0; 4]);
        assert_eq!(pixel(&bytes, 25, 12), [0, 255, 0, 255]);
        assert_eq!(pixel(&bytes, 26, 12), [0, 255, 0, 255]);
        assert_eq!(pixel(&bytes, 27, 12), [0; 4]);
        assert_eq!(pixel(&bytes, 44, 12), [0, 0, 255, 255]);
        assert_eq!(pixel(&bytes, 45, 12), [0, 0, 255, 255]);
        assert_eq!(pixel(&bytes, 46, 12), [0; 4]);
        near(pixel(&bytes, 6, 26), [128, 0, 0, 128]);
        near(pixel(&bytes, 7, 27), [128, 0, 0, 128]);
        assert_eq!(pixel(&bytes, 12, 32), [0; 4]);
        assert_eq!(pixel(&bytes, 26, 26), [0; 4]);
        assert_eq!(pixel(&bytes, 34, 26), [0, 255, 0, 255]);
        assert_eq!(pixel(&bytes, 34, 34), [0; 4]);
        assert_eq!(pixel(&bytes, 52, 32), [0, 0, 255, 255]);
        near(pixel(&bytes, 10, 50), [64, 0, 0, 64]);
        near(pixel(&bytes, 30, 50), [0, 64, 0, 64]);
        near(pixel(&bytes, 50, 50), [0, 0, 64, 64]);
        assert!(errors.pop().await.is_none());
    });
}

// Independent geometric oracle: dense boundary samples, rather than the shader's
// root solver. Hard-edge comparisons omit samples close to either stroke boundary.
fn ellipse_reference_distance(point: [f64; 2], axes: [f64; 2]) -> f64 {
    let distance = (0..4096)
        .map(|i| {
            let angle = i as f64 * std::f64::consts::TAU / 4096.;
            (point[0] - axes[0] * angle.cos()).hypot(point[1] - axes[1] * angle.sin())
        })
        .fold(f64::INFINITY, f64::min);
    if (point[0] / axes[0]).powi(2) + (point[1] / axes[1]).powi(2) <= 1. {
        -distance
    } else {
        distance
    }
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn ellipse_outlines_offset_by_distance_including_axis_points_and_eccentricity() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let mut t = target(&g, 1, false);
        let mut shapes = ShapeRenderer::new(&g);
        for axes in [[20., 8.], [8., 20.], [20., 1.], [12., 12.]] {
            for stroke in [
                Stroke::new(3.).inside(),
                Stroke::new(3.),
                Stroke::new(3.).outside(),
                Stroke::new(40.).inside(),
            ] {
                // Half-pixel center deliberately exercises the solver's axis cases.
                let bytes = pixels(&g, &mut t, |f| {
                    shapes
                        .draw(
                            &mut f.render_pass().begin().unwrap(),
                            ShapeDraw::ellipse(
                                Rect::new(
                                    32.5 - axes[0],
                                    32.5 - axes[1],
                                    axes[0] * 2.,
                                    axes[1] * 2.,
                                ),
                                RED,
                            )
                            .stroke(stroke)
                            .antialiasing(EdgeAntialiasing::None),
                        )
                        .unwrap();
                });
                let (inset, outset) = stroke.offsets();
                for y in 0..64 {
                    for x in 0..64 {
                        let d = ellipse_reference_distance(
                            [x as f64 - 32., y as f64 - 32.],
                            axes.map(f64::from),
                        );
                        if (d + f64::from(inset)).abs() < 0.06
                            || (d - f64::from(outset)).abs() < 0.06
                        {
                            continue;
                        }
                        let expected = if d > -f64::from(inset) && d < f64::from(outset) {
                            [255, 0, 0, 255]
                        } else {
                            [0; 4]
                        };
                        assert_eq!(
                            pixel(&bytes, x, y),
                            expected,
                            "axes={axes:?} stroke={stroke:?} pixel=({x},{y}) distance={d}"
                        );
                    }
                }
            }
        }
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn mixed_fill_outline_batches_are_atomic_reuse_storage_and_match_single_draws() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let mut t = target(&g, 4, true);
        let mut shapes = ShapeRenderer::new(&g);
        shapes.prepare(&t.render_format()).unwrap();
        let pipeline = shapes.inner.pipeline(&t.render_format()).unwrap();
        let draws = [
            ShapeDraw::rect(Rect::new(2., 2., 60., 60.), BLUE),
            ShapeDraw::rounded_rect(Rect::new(8., 8., 40., 40.), 7., [1., 0., 0., 0.5])
                .stroke(Stroke::new(2.5)),
            ShapeDraw::ellipse(Rect::new(16., 16., 32., 20.), [0., 1., 0., 0.5])
                .stroke(Stroke::new(4.).outside()),
            ShapeDraw::rect(Rect::new(0.1, 0.2, 0.3, 0.2), RED)
                .stroke(Stroke::new(0.02).inside())
                .space(DrawSpace::Normalized)
                .transform(Transform2D::translation(0.1, 0.1)),
        ];
        let render = |f: &mut crate::Frame<'_, '_>, s: &mut ShapeRenderer, batch: bool| {
            drop(f.render_pass().begin().unwrap());
            let mut p = f
                .render_pass()
                .load_all()
                .depth_ops(None)
                .stencil_ops(None)
                .begin()
                .unwrap();
            p.set_scissor_rect(6, 6, 52, 52).unwrap();
            if batch {
                s.draw_many(&mut p, &draws).unwrap();
            } else {
                for d in draws {
                    s.draw(&mut p, d).unwrap();
                }
            }
        };
        let single = pixels(&g, &mut t, |f| render(f, &mut shapes, false));
        let batch = pixels(&g, &mut t, |f| render(f, &mut shapes, true));
        assert_eq!(single, batch);
        let capacity = shapes.inner.parameters.capacity();
        let buffers: Vec<_> = g
            .upload_pool
            .lock()
            .unwrap()
            .iter()
            .map(|(b, _)| b.clone())
            .collect();
        pixels(&g, &mut t, |f| render(f, &mut shapes, true));
        assert_eq!(shapes.inner.parameters.capacity(), capacity);
        assert_eq!(shapes.inner.pipeline(&t.render_format()).unwrap(), pipeline);
        assert!(
            g.upload_pool
                .lock()
                .unwrap()
                .iter()
                .all(|(b, _)| buffers.contains(b))
        );
        let bytes = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            for width in [-1., f32::NAN, f32::INFINITY] {
                let bad =
                    ShapeDraw::rect(Rect::new(0., 0., 64., 64.), RED).stroke(Stroke::new(width));
                assert!(matches!(
                    shapes.draw_many(&mut p, &[draws[0], bad]),
                    Err(Error::InvalidShapeDraw)
                ));
            }
            let overflow = ShapeDraw::rect(Rect::new(0., 0., f32::MAX, f32::MAX), RED)
                .stroke(Stroke::new(f32::MAX).outside());
            assert!(matches!(
                shapes.draw(&mut p, overflow),
                Err(Error::InvalidShapeDraw)
            ));
            for d in [
                ShapeDraw::rect(Rect::new(0., 0., 64., 64.), RED).stroke(Stroke::new(0.)),
                ShapeDraw::ellipse(Rect::new(0., 0., 0., 64.), RED)
                    .stroke(Stroke::new(4.).outside()),
                ShapeDraw::rounded_rect(Rect::new(0., 0., 64., 64.), 8., RED)
                    .stroke(Stroke::new(4.))
                    .transform(Transform2D::scale(0., 1.)),
            ] {
                shapes.draw(&mut p, d).unwrap();
            }
            shapes
                .draw(
                    &mut p,
                    ShapeDraw::rect(Rect::new(48., 48., 8., 8.), GREEN)
                        .stroke(Stroke::new(2.).inside()),
                )
                .unwrap();
        });
        assert_eq!(pixel(&bytes, 32, 32), [0; 4]);
        assert_eq!(pixel(&bytes, 48, 52), [0, 255, 0, 255]);
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn zero_corner_radius_matches_sharp_rectangles_for_every_stroke_placement() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let mut t = target(&g, 1, false);
        let mut shapes = ShapeRenderer::new(&g);
        for stroke in [
            Stroke::new(3.).inside(),
            Stroke::new(3.),
            Stroke::new(3.).outside(),
        ] {
            let rect = Rect::new(8.5, 8.5, 40., 32.);
            let sharp = pixels(&g, &mut t, |f| {
                shapes
                    .draw(
                        &mut f.render_pass().begin().unwrap(),
                        ShapeDraw::rect(rect, RED).stroke(stroke),
                    )
                    .unwrap();
            });
            let zero_radius = pixels(&g, &mut t, |f| {
                shapes
                    .draw(
                        &mut f.render_pass().begin().unwrap(),
                        ShapeDraw::rounded_rect(rect, 0., RED).stroke(stroke),
                    )
                    .unwrap();
            });
            assert_eq!(sharp, zero_radius);
        }
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn independent_corner_radii_fill_and_outline_each_corner() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut t = target(&g, 1, false);
        let mut shapes = ShapeRenderer::new(&g);
        // Top-left 16, top-right sharp, bottom-right 8, bottom-left sharp.
        let radii = crate::CornerRadii::new(16., 0., 8., 0.);
        let bytes = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            shapes
                .draw_many(
                    &mut p,
                    &[
                        ShapeDraw::rounded_rect_corners(Rect::new(0., 0., 32., 32.), radii, RED),
                        ShapeDraw::rounded_rect_corners(Rect::new(32., 0., 32., 32.), radii, GREEN)
                            .stroke(Stroke::new(2.).inside()),
                        // Oversized radii scale together and stay within half the shorter side.
                        ShapeDraw::rounded_rect_corners(
                            Rect::new(0., 40., 64., 24.),
                            crate::CornerRadii::new(48., 48., 0., 0.),
                            BLUE,
                        ),
                    ],
                )
                .unwrap();
        });
        // Fill: rounded corners are open, sharp corners are covered.
        assert_eq!(pixel(&bytes, 1, 1), [0; 4]);
        assert_eq!(pixel(&bytes, 3, 3), [0; 4]);
        assert_eq!(pixel(&bytes, 31, 0), [255, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 0, 31), [255, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 31, 31), [0; 4]);
        assert_eq!(pixel(&bytes, 28, 28), [255, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 16, 16), [255, 0, 0, 255]);
        // Outline: the sharp top-right corner stays square, rounded corners follow the arc.
        assert_eq!(pixel(&bytes, 63, 0), [0, 255, 0, 255]);
        assert_eq!(pixel(&bytes, 32, 31), [0, 255, 0, 255]);
        assert_eq!(pixel(&bytes, 33, 1), [0; 4]);
        assert_eq!(pixel(&bytes, 48, 16), [0; 4]);
        assert_eq!(pixel(&bytes, 48, 0), [0, 255, 0, 255]);
        // Both top radii become 12, half the height; the bottom stays sharp.
        assert_eq!(pixel(&bytes, 2, 42), [0; 4]);
        assert_eq!(pixel(&bytes, 32, 41), [0, 0, 255, 255]);
        assert_eq!(pixel(&bytes, 0, 63), [0, 0, 255, 255]);
        assert_eq!(pixel(&bytes, 63, 63), [0, 0, 255, 255]);
        assert!(errors.pop().await.is_none());
    });
}

// Exact Gaussian-blurred rectangle: separable products of normal CDF differences.
fn blurred_rect(x: f64, y: f64, rect: [f64; 4], sigma: f64) -> f64 {
    fn phi(z: f64) -> f64 {
        // Abramowitz-Stegun 7.1.26 erf, accurate to 1.5e-7.
        let t = 1. / (1. + 0.3275911 * (z.abs() / 2f64.sqrt()));
        let poly = t
            * (0.254829592
                + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
        let erf = 1. - poly * (-(z * z) / 2.).exp();
        0.5 * (1. + erf.copysign(z))
    }
    let [left, top, width, height] = rect;
    (phi((x - left) / sigma) - phi((x - left - width) / sigma))
        * (phi((y - top) / sigma) - phi((y - top - height) / sigma))
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn box_shadows_match_gaussian_blur_and_follow_offset_spread_and_corners() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut t = target(&g, 1, false);
        let mut shapes = ShapeRenderer::new(&g);
        let white = [1.; 4];
        // Blur 8 is a standard deviation of 4; the shadow moves by its offset.
        let blurred = pixels(&g, &mut t, |f| {
            shapes
                .draw(
                    &mut f.render_pass().begin().unwrap(),
                    ShapeDraw::box_shadow(
                        Rect::new(18., 20., 20., 16.),
                        crate::CornerRadii::ZERO,
                        crate::BoxShadow::new(white).offset(4., 4.).blur(8.),
                    ),
                )
                .unwrap();
        });
        let mut worst = 0f64;
        for y in 0..64 {
            for x in 0..64 {
                let expected =
                    blurred_rect(x as f64 + 0.5, y as f64 + 0.5, [22., 24., 20., 16.], 4.);
                let actual = f64::from(pixel(&blurred, x, y)[3]) / 255.;
                worst = worst.max((expected - actual).abs());
            }
        }
        assert!(worst * 255. <= 4., "max error {} / 255", worst * 255.);
        assert_eq!(pixel(&blurred, 63, 63), [0; 4]);
        // Zero blur with spread is the sharp grown rectangle.
        let mut sharp = |draw| {
            pixels(&g, &mut t, |f| {
                shapes
                    .draw(&mut f.render_pass().begin().unwrap(), draw)
                    .unwrap();
            })
        };
        assert_eq!(
            sharp(ShapeDraw::box_shadow(
                Rect::new(10.5, 12., 20., 10.),
                crate::CornerRadii::ZERO,
                crate::BoxShadow::new(RED).offset(3., -2.).spread(2.),
            )),
            sharp(ShapeDraw::rect(Rect::new(11.5, 8., 24., 14.), RED))
        );
        // Spread grows nonzero radii; a sharp zero blur matches the rounded shape.
        assert_eq!(
            sharp(ShapeDraw::box_shadow(
                Rect::new(10., 10., 30., 30.),
                crate::CornerRadii::new(6., 0., 6., 0.),
                crate::BoxShadow::new(GREEN).spread(4.),
            )),
            sharp(ShapeDraw::rounded_rect_corners(
                Rect::new(6., 6., 38., 38.),
                crate::CornerRadii::new(10., 0., 10., 0.),
                GREEN
            ))
        );
        // Only the rounded top-left corner of the shape softens its blurred corner.
        let corners = sharp(ShapeDraw::box_shadow(
            Rect::new(12., 12., 40., 40.),
            crate::CornerRadii::new(16., 0., 0., 0.),
            crate::BoxShadow::new(white).blur(6.),
        ));
        let (rounded, square) = (pixel(&corners, 14, 14)[3], pixel(&corners, 49, 14)[3]);
        assert!(rounded + 40 < square, "{rounded} {square}");
        assert_eq!(pixel(&corners, 31, 49), pixel(&corners, 32, 49));
        assert!(errors.pop().await.is_none());
        let shadow = ShapeDraw::box_shadow(
            Rect::new(0., 0., 4., 4.),
            crate::CornerRadii::ZERO,
            crate::BoxShadow::new(RED).blur(-1.),
        );
        assert!(matches!(shadow.validate(), Err(Error::InvalidShapeDraw)));
        let shadow = ShapeDraw::box_shadow(
            Rect::new(0., 0., 4., 4.),
            crate::CornerRadii::ZERO,
            crate::BoxShadow::new(RED).spread(f32::NAN),
        );
        assert!(shadow.validate().is_err());
        assert!(
            ShapeDraw::box_shadow(
                Rect::new(0., 0., 4., 4.),
                crate::CornerRadii::ZERO,
                crate::BoxShadow::new(RED)
            )
            .stroke(Stroke::new(1.))
            .validate()
            .is_err()
        );
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn consecutive_draws_merge_in_order_and_flush_before_other_commands() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut t = target(&g, 1, false);
        let mut shapes = ShapeRenderer::new(&g);
        shapes.prepare(&t.render_format()).unwrap();
        // Overlapping translucent draws: any reordering changes the blend result.
        let draws: Vec<_> = (0..40)
            .map(|i| {
                let v = i as f32 / 40.;
                ShapeDraw::rounded_rect(
                    Rect::new((i % 8) as f32 * 6., (i / 8) as f32 * 9., 14., 14.),
                    3.,
                    [v, 0.5, 1. - v, 0.6],
                )
            })
            .collect();
        let batch = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            shapes.draw_many(&mut p, &draws).unwrap();
        });
        let individual = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            for &draw in &draws {
                shapes.draw(&mut p, draw).unwrap();
            }
            p.flush();
            assert_eq!(p.recorded_draws, 1);
        });
        assert_eq!(batch, individual);

        // A state change between draws records the first before the change.
        let bytes = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            shapes
                .draw(&mut p, ShapeDraw::rect(Rect::new(0., 0., 64., 64.), RED))
                .unwrap();
            p.set_scissor_rect(0, 0, 32, 64).unwrap();
            shapes
                .draw(&mut p, ShapeDraw::rect(Rect::new(0., 0., 64., 64.), BLUE))
                .unwrap();
            p.flush();
            assert_eq!(p.recorded_draws, 2);
        });
        near(pixel(&bytes, 10, 10), [0, 0, 255, 255]);
        near(pixel(&bytes, 50, 10), [255, 0, 0, 255]);

        // Raw access records the pending draw first, so raw state cannot leak into it.
        let bytes = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            shapes
                .draw(&mut p, ShapeDraw::rect(Rect::new(0., 0., 64., 64.), GREEN))
                .unwrap();
            p.as_wgpu().set_scissor_rect(0, 0, 1, 1);
        });
        near(pixel(&bytes, 40, 40), [0, 255, 0, 255]);
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn session_clips_restore_after_pass_raw_custom_and_nested_scope_access() {
    pollster::block_on(async {
        let base = GraphicsContext::headless().await.unwrap();
        for capacity in [0, 2, crate::MAX_ROUNDED_CLIPS] {
            let g = if capacity == crate::MAX_ROUNDED_CLIPS {
                base.clone()
            } else {
                let (device, queue) = base
                    .adapter()
                    .request_device(&wgpu::DeviceDescriptor {
                        required_limits: wgpu::Limits {
                            max_uniform_buffer_binding_size: 128,
                            max_uniform_buffers_per_shader_stage: if capacity == 0 { 1 } else { 2 },
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                    .await
                    .unwrap();
                GraphicsContext::from_wgpu(
                    base.instance().clone(),
                    base.adapter().clone(),
                    device,
                    queue,
                )
            };
            assert_eq!(g.rounded_clip_capacity(), capacity);
            let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
            let mut t = target(&g, 1, false);
            let mut renderer = PrimitiveRenderer::new(&g, Default::default());
            let mut painter = crate::Painter::new(&g);
            let brush = g
                .create_brush(crate::BrushOptions::solid([0.3, 0.8, 1., 1.]))
                .unwrap();
            let other_brush = g.create_brush(crate::BrushOptions::solid(RED)).unwrap();
            let stride = g.device().limits().min_uniform_buffer_offset_alignment as u64;
            let buffer = g.device().create_buffer(&wgpu::BufferDescriptor {
                label: Some("session application dynamic uniform"),
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
            let shader = g.device().create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("session custom renderer"),
                source: wgpu::ShaderSource::Wgsl("@vertex fn vs(@location(0) p:vec3<f32>)->@builtin(position) vec4<f32>{return vec4(p,1.0);}\n@fragment fn fs()->@location(0) vec4<f32>{return vec4(0.0,0.0,1.0,0.1);}".into()),
            });
            let material =
                g.create_material(MaterialOptions::new(&shader).entry_points("vs", "fs"));
            let mesh = g
                .create_mesh(
                    &[
                        Vertex::new([-1., -1., 0.], BLUE),
                        Vertex::new([3., -1., 0.], BLUE),
                        Vertex::new([-1., 3., 0.], BLUE),
                    ],
                    &[0, 1, 2],
                )
                .unwrap();
            let mut meshes = MeshRenderer::new(&g);
            for brushed in [false, true] {
                for line in [false, true] {
                    let mut render = |f: &mut crate::Frame<'_, '_>, disrupt: bool| {
                        let mut pass = f.render_pass().begin().unwrap();
                        if capacity > 0 {
                            pass.set_rounded_clips(&[crate::RoundedClip::new(
                                Rect::new(4., 4., 56., 56.),
                                crate::CornerRadii::uniform(16.),
                            )])
                            .unwrap();
                        }
                        let before = g.upload_stats();
                        let mut s = renderer.bind(&mut pass, brushed.then_some(&brush)).unwrap();
                        // No clip uploads or binding creation on empty/invalid work,
                        // including after pass access has invalidated the session.
                        for _ in 0..2 {
                            s.draw_many::<ShapeDraw>(&[]).unwrap();
                            assert!(
                                s.draw(ShapeDraw::rect(Rect::new(0., 0., -1., 1.), RED))
                                    .is_err()
                            );
                            assert!(
                                s.draw_many(&[
                                    ShapeDraw::rect(Rect::new(0., 0., 8., 8.), RED),
                                    ShapeDraw::ellipse(Rect::new(0., 0., -1., 1.), RED),
                                ])
                                .is_err()
                            );
                            assert_eq!(
                                g.upload_stats().clip_bindings_created,
                                before.clip_bindings_created
                            );
                            s.pass();
                        }
                        for phase in 0..4 {
                            s.pass()
                                .set_viewport(
                                    0.,
                                    0.,
                                    if phase % 2 == 0 { 64. } else { 60. },
                                    64.,
                                    0.,
                                    1.,
                                )
                                .unwrap();
                            let clip = crate::RoundedClip::new(
                                Rect::new(4., 4., 56., 56.),
                                crate::CornerRadii::uniform(16.),
                            );
                            s.pass()
                                .set_rounded_clips(&vec![
                                    clip;
                                    if capacity == 0 {
                                        0
                                    } else {
                                        [0, 2, 1, 0][phase]
                                    }
                                ])
                                .unwrap();
                            // A custom material changes pipeline/vertex state, while
                            // a nested Painter scope temporarily changes the stack.
                            meshes
                                .draw_with_material(s.pass(), &mesh, &material)
                                .unwrap();
                            if capacity > 0 && phase != 1 {
                                let mut paint = painter.begin(s.pass()).unwrap();
                                let mut child = paint
                                    .clipped_rounded(
                                        Rect::new(12., 12., 40., 40.),
                                        crate::CornerRadii::uniform(8.),
                                    )
                                    .unwrap();
                                child.fill_rect(Rect::new(16., 16., 8., 8.), GREEN).unwrap();
                            }
                            for i in 0..4 {
                                if disrupt {
                                    let p = s.pass();
                                    match i {
                                        0 | 1 => p.set_bind_group(
                                            1,
                                            &group,
                                            &[if i == 0 { 0 } else { stride as u32 }],
                                        ),
                                        2 => {
                                            p.as_wgpu().set_bind_group(1, &group, &[0]);
                                            p.as_wgpu().set_vertex_buffer(1, g.no_clip().slice(..));
                                            p.as_wgpu().set_scissor_rect(0, 0, 1, 1);
                                        }
                                        _ => other_brush.bind(p),
                                    }
                                }
                                let color = [0.2, 0.4, 0.8, 0.4];
                                // Two draws per restoration exercise the amortized path
                                // and translucent order across clip transitions.
                                for j in 0..2 {
                                    if !disrupt {
                                        // Reference explicitly binds both clip interfaces
                                        // for every draw, independently of Session caching.
                                        s.pass().bind_clip(1);
                                        s.pass().bind_rounded_clips();
                                    }
                                    let x = (phase * 16 + i * 3 + j) as f32;
                                    if line {
                                        s.draw(LineDraw::new([x, 0.], [x, 64.], color).width(5.))
                                            .unwrap();
                                    } else {
                                        s.draw(ShapeDraw::rect(Rect::new(x, 0., 5., 64.), color))
                                            .unwrap();
                                    }
                                }
                            }
                        }
                    };
                    let expected = pixels(&g, &mut t, |f| render(f, false));
                    let actual = pixels(&g, &mut t, |f| render(f, true));
                    assert_eq!(
                        actual, expected,
                        "capacity={capacity} brushed={brushed} line={line}"
                    );
                    assert!(pixel(&actual, 50, 32)[3] > 0);
                    if capacity > 0 {
                        assert_ne!(pixel(&actual, 25, 0), pixel(&actual, 25, 32));
                    }
                }
            }
            assert!(errors.pop().await.is_none(), "capacity={capacity}");
        }
    });
}
