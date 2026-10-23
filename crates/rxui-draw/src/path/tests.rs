use super::*;
use crate::{
    Framebuffer, FramebufferOptions, GraphicsContext, Painter, PipelineOptions, Rect, ShapeDraw,
    ShapeRenderer, frame::Frame, framebuffer::tests::pixels, wgpu,
};

fn rectangle(builder: &mut PathBuilder, x: f32, y: f32, size: f32, reverse: bool) {
    builder.move_to([x, y]);
    if reverse {
        builder
            .line_to([x, y + size])
            .line_to([x + size, y + size])
            .line_to([x + size, y]);
    } else {
        builder
            .line_to([x + size, y])
            .line_to([x + size, y + size])
            .line_to([x, y + size]);
    }
    builder.close();
}
fn box_path(x: f32, y: f32, size: f32) -> Path {
    let mut builder = Path::builder();
    rectangle(&mut builder, x, y, size, false);
    builder.build().unwrap()
}
fn target(g: &GraphicsContext, samples: u32, stencil: bool) -> Framebuffer {
    let mut options = FramebufferOptions::new(64, 64)
        .sample_count(samples)
        .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
    if stencil {
        options = options.depth_stencil(wgpu::TextureFormat::Depth24PlusStencil8);
    }
    g.create_framebuffer(options).unwrap()
}
fn pixel(bytes: &[u8], x: usize, y: usize) -> [u8; 4] {
    bytes[(y * 64 + x) * 4..(y * 64 + x + 1) * 4]
        .try_into()
        .unwrap()
}
fn hard(color: [f32; 4]) -> PathDraw {
    PathDraw::new(color).antialiasing(EdgeAntialiasing::None)
}
fn area(geometry: &geometry::Geometry) -> f64 {
    geometry.indices[..geometry.interior_indices as usize]
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| {
            let [a, b, c] = [
                geometry.vertices[t[0] as usize].position,
                geometry.vertices[t[1] as usize].position,
                geometry.vertices[t[2] as usize].position,
            ];
            ((f64::from(b[0]) - f64::from(a[0])) * (f64::from(c[1]) - f64::from(a[1]))
                - (f64::from(b[1]) - f64::from(a[1])) * (f64::from(c[0]) - f64::from(a[0])))
                * 0.5
        })
        .sum()
}

#[test]
fn command_validation_and_empty_paths_are_explicit() {
    for mut b in [Path::builder(), Path::builder()] {
        b.line_to([1., 1.]);
        assert!(matches!(b.build(), Err(Error::InvalidPath { command: 0 })));
    }
    let mut b = Path::builder();
    b.move_to([0., 0.]).close().line_to([1., 1.]);
    assert!(matches!(b.build(), Err(Error::InvalidPath { command: 2 })));
    let mut b = Path::builder();
    b.move_to([0., 0.])
        .cubic_to([1., 0.], [f32::NAN, 2.], [0., 3.]);
    assert!(matches!(b.build(), Err(Error::InvalidPath { command: 1 })));
    assert!(Path::builder().build().unwrap().is_empty());
    let mut b = Path::builder();
    b.move_to([1., 1.]).move_to([2., 2.]);
    assert!(b.build().unwrap().is_empty());
}

#[test]
fn fill_rules_holes_open_contours_and_intersections_tessellate() {
    let mut tess = geometry::Tessellators::default();
    for (reverse, rule, expected) in [
        (false, FillRule::NonZero, 1600.),
        (true, FillRule::NonZero, 1200.),
        (false, FillRule::EvenOdd, 1200.),
    ] {
        let mut b = Path::builder();
        rectangle(&mut b, 4., 4., 40., false);
        rectangle(&mut b, 14., 14., 20., reverse);
        let g = tess
            .prepare(&b.build().unwrap(), PathOptions::new().fill_rule(rule))
            .unwrap();
        assert!(
            (area(&g) - expected).abs() < 0.01,
            "area {} != {expected}",
            area(&g)
        );
    }
    let mut b = Path::builder();
    b.move_to([0., 0.])
        .line_to([10., 10.])
        .line_to([0., 10.])
        .line_to([10., 0.]);
    let bow = tess
        .prepare(
            &b.build().unwrap(),
            PathOptions::new().fill_rule(FillRule::EvenOdd),
        )
        .unwrap();
    assert!((area(&bow) - 50.).abs() < 0.01);
    let mut b = Path::builder();
    b.move_to([0., 0.]).line_to([10., 0.]).line_to([0., 10.]);
    let open = tess
        .prepare(&b.build().unwrap(), PathOptions::new())
        .unwrap();
    assert!((area(&open) - 50.).abs() < 0.01);
    // Separate contours touching at a vertex remain usable.
    let mut b = Path::builder();
    rectangle(&mut b, 0., 0., 10., false);
    rectangle(&mut b, 10., 10., 10., false);
    let touching = tess
        .prepare(&b.build().unwrap(), PathOptions::new())
        .unwrap();
    assert!((area(&touching) - 200.).abs() < 0.01);
}

#[test]
fn curve_tolerance_and_stroke_union_affect_only_preparation() {
    let mut b = Path::builder();
    b.move_to([0., 0.])
        .quadratic_to([20., -30.], [40., 0.])
        .cubic_to([80., 0.], [80., 40.], [40., 40.])
        .line_to([0., 40.])
        .close();
    let path = b.build().unwrap();
    let mut tess = geometry::Tessellators::default();
    let coarse = tess
        .prepare(&path, PathOptions::new().tolerance(2.))
        .unwrap();
    let fine = tess
        .prepare(&path, PathOptions::new().tolerance(0.02))
        .unwrap();
    assert!(fine.vertices.len() > coarse.vertices.len());
    for tolerance in [0., -1., f32::NAN, f32::INFINITY] {
        assert!(matches!(
            tess.prepare(&path, PathOptions::new().tolerance(tolerance)),
            Err(Error::InvalidPathOptions)
        ));
    }
    for stroke in [
        PathStroke::new(-1.),
        PathStroke::new(f32::NAN),
        PathStroke::new(1.).miter_limit(0.5),
    ] {
        assert!(matches!(
            tess.prepare(&path, PathOptions::new().stroke(stroke)),
            Err(Error::InvalidPathOptions)
        ));
    }
    assert!(
        tess.prepare(&path, PathOptions::new().stroke(PathStroke::new(0.)))
            .unwrap()
            .indices
            .is_empty()
    );
    let mut b = Path::builder();
    b.move_to([4., 32.])
        .line_to([60., 32.])
        .move_to([32., 4.])
        .line_to([32., 60.]);
    let cross = tess
        .prepare(
            &b.build().unwrap(),
            PathOptions::new().stroke(PathStroke::new(8.)),
        )
        .unwrap();
    assert!(
        (area(&cross) - (56. * 8. * 2. - 8. * 8.)).abs() < 0.01,
        "cross area {}",
        area(&cross)
    );
    for join in [LineJoin::Miter, LineJoin::Bevel, LineJoin::Round] {
        for cap in [LineCap::Butt, LineCap::Square, LineCap::Round] {
            let g = tess
                .prepare(
                    &path,
                    PathOptions::new().stroke(PathStroke::new(8.).join(join).cap(cap)),
                )
                .unwrap();
            assert!(area(&g) > 0.);
        }
    }
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn filled_holes_and_translucent_strokes_have_expected_pixels() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut renderer = PathRenderer::new(&g);
        let mut b = Path::builder();
        rectangle(&mut b, 4., 4., 56., false);
        rectangle(&mut b, 20., 20., 24., false);
        let path = b.build().unwrap();
        let hole = renderer
            .prepare_path(&path, PathOptions::new().fill_rule(FillRule::EvenOdd))
            .unwrap();
        let solid = renderer.prepare_path(&path, PathOptions::new()).unwrap();
        let mut b = Path::builder();
        b.move_to([4., 32.])
            .line_to([60., 32.])
            .move_to([32., 4.])
            .line_to([32., 60.]);
        let cross = renderer
            .prepare_path(
                &b.build().unwrap(),
                PathOptions::new().stroke(PathStroke::new(8.)),
            )
            .unwrap();
        let mut t = target(&g, 1, false);
        for (path, has_hole) in [(&hole, true), (&solid, false)] {
            let bytes = pixels(&g, &mut t, |f| {
                renderer
                    .draw(
                        &mut f.render_pass().begin().unwrap(),
                        path,
                        hard([1., 0., 0., 1.]),
                    )
                    .unwrap()
            });
            assert_eq!(pixel(&bytes, 10, 10), [255, 0, 0, 255]);
            assert_eq!(
                pixel(&bytes, 32, 32),
                if has_hole { [0; 4] } else { [255, 0, 0, 255] }
            );
        }
        let bytes = pixels(&g, &mut t, |f| {
            renderer
                .draw(
                    &mut f.render_pass().begin().unwrap(),
                    &cross,
                    PathDraw::new([0., 1., 0., 0.5]),
                )
                .unwrap()
        });
        assert_eq!(pixel(&bytes, 32, 32), [0, 128, 0, 128]);
        assert_eq!(pixel(&bytes, 12, 32), [0, 128, 0, 128]);
        assert_eq!(pixel(&bytes, 32, 12), [0, 128, 0, 128]);
        assert_eq!(pixel(&bytes, 12, 12), [0; 4]);
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn batches_scopes_and_painter_match_and_restore_after_other_renderers() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut renderer = PathRenderer::new(&g);
        let mut painter = Painter::new(&g);
        let mut shapes = ShapeRenderer::new(&g);
        let path = renderer
            .prepare_path(&box_path(0., 0., 8.), PathOptions::new())
            .unwrap();
        let mut t = target(&g, 1, false);
        renderer.prepare(&t.render_format()).unwrap();
        painter.prepare(&t.render_format()).unwrap();
        let draws: Vec<_> = (0..2050)
            .map(|i| {
                PathDraw::new(if i == 2049 {
                    [0., 0., 1., 0.5]
                } else {
                    [1., 0., 0., 0.1]
                })
                .transform(Transform2D::translation(
                    (i % 7) as f32 * 7. + 2.,
                    ((i / 7) % 7) as f32 * 7. + 2.,
                ))
            })
            .collect();
        let individual = pixels(&g, &mut t, |f| {
            let mut pass = f.render_pass().begin().unwrap();
            for &draw in &draws {
                renderer.draw(&mut pass, &path, draw).unwrap();
            }
        });
        let batch = pixels(&g, &mut t, |f| {
            renderer
                .draw_many(&mut f.render_pass().begin().unwrap(), &path, &draws)
                .unwrap()
        });
        assert_eq!(individual, batch);
        let scoped = pixels(&g, &mut t, |f| {
            let mut pass = f.render_pass().begin().unwrap();
            let mut scope = renderer.bind(&mut pass).unwrap();
            scope.draw(&path, draws[0]).unwrap();
            scope.pass().as_wgpu().set_scissor_rect(0, 0, 1, 1);
            shapes
                .draw(
                    scope.pass(),
                    ShapeDraw::rect(Rect::new(60., 60., 4., 4.), [1.; 4]),
                )
                .unwrap();
            for &draw in &draws[1..] {
                scope.draw(&path, draw).unwrap();
            }
        });
        let expected = pixels(&g, &mut t, |f| {
            let mut pass = f.render_pass().begin().unwrap();
            renderer.draw(&mut pass, &path, draws[0]).unwrap();
            shapes
                .draw(
                    &mut pass,
                    ShapeDraw::rect(Rect::new(60., 60., 4., 4.), [1.; 4]),
                )
                .unwrap();
            renderer.draw_many(&mut pass, &path, &draws[1..]).unwrap();
        });
        assert_eq!(scoped, expected);
        let parent = Transform2D::scale(0.7, 0.8).then(Transform2D::translation(5., 4.));
        let painted = pixels(&g, &mut t, |f| {
            let mut pass = f.render_pass().begin().unwrap();
            let mut paint = painter.begin(&mut pass).unwrap();
            paint
                .transformed(parent)
                .unwrap()
                .draw_paths(&path, &draws)
                .unwrap();
        });
        let transformed: Vec<_> = draws
            .iter()
            .map(|d| d.transform(d.transform.then(parent)))
            .collect();
        let expected = pixels(&g, &mut t, |f| {
            renderer
                .draw_many(&mut f.render_pass().begin().unwrap(), &path, &transformed)
                .unwrap()
        });
        assert_eq!(painted, expected);
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn draw_errors_are_atomic_and_prepared_paths_are_device_bound() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let other = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut renderer = PathRenderer::new(&g);
        let mut foreign = PathRenderer::new(&other);
        let source = box_path(4., 4., 32.);
        let path = renderer.prepare_path(&source, PathOptions::new()).unwrap();
        let foreign = foreign.prepare_path(&source, PathOptions::new()).unwrap();
        let empty = renderer
            .prepare_path(&Path::builder().build().unwrap(), PathOptions::new())
            .unwrap();
        assert!(empty.is_empty());
        assert_eq!(empty.geometry_bytes(), 0);
        let mut t = target(&g, 1, false);
        let bytes = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            assert!(matches!(
                renderer.draw(&mut p, &foreign, PathDraw::default()),
                Err(Error::DeviceMismatch)
            ));
            for invalid in [
                PathDraw::new([1., 0., 0., 2.]),
                PathDraw::default().transform([f32::NAN, 0., 0., 1., 0., 0.]),
                PathDraw::default().transform(Transform2D::scale(f32::MAX, f32::MAX)),
            ] {
                assert!(matches!(
                    renderer.draw_many(&mut p, &path, &[PathDraw::default(), invalid]),
                    Err(Error::InvalidPathDraw)
                ));
            }
            renderer
                .draw(
                    &mut p,
                    &path,
                    hard([0., 1., 0., 1.]).transform(Transform2D::scale(0., 1.)),
                )
                .unwrap();
            renderer.draw(&mut p, &empty, PathDraw::default()).unwrap();
            renderer
                .draw(
                    &mut p,
                    &path,
                    hard([0., 0., 1., 1.]).transform(Transform2D::translation(32., 32.)),
                )
                .unwrap();
        });
        assert_eq!(pixel(&bytes, 8, 8), [0; 4]);
        assert_eq!(pixel(&bytes, 40, 40), [0, 0, 255, 255]);
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn coverage_transforms_viewport_clipping_and_msaa_work() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut renderer = PathRenderer::new(&g);
        let path = renderer
            .prepare_path(&box_path(0., 0., 1.), PathOptions::new())
            .unwrap();
        for samples in [1, 4] {
            if !target(&g, 1, false)
                .supported_sample_counts()
                .contains(&samples)
            {
                continue;
            }
            let mut t = target(&g, samples, false);
            let bytes = pixels(&g, &mut t, |f| {
                let mut p = f.render_pass().begin().unwrap();
                p.set_viewport(8., 12., 40., 32., 0., 1.).unwrap();
                p.set_scissor_rect(10, 12, 30, 32).unwrap();
                renderer
                    .draw(
                        &mut p,
                        &path,
                        hard([1., 0., 0., 1.])
                            .space(DrawSpace::Normalized)
                            .transform(Transform2D::scale(0.5, 0.5)),
                    )
                    .unwrap();
                renderer
                    .draw(
                        &mut p,
                        &path,
                        PathDraw::new([0., 1., 0., 1.]).transform(
                            Transform2D::scale(-8., 8.).then(Transform2D::translation(32.25, 16.)),
                        ),
                    )
                    .unwrap();
            });
            assert_eq!(pixel(&bytes, 9, 16), [0; 4]);
            assert_eq!(pixel(&bytes, 12, 16), [255, 0, 0, 255]);
            assert_eq!(pixel(&bytes, 38, 32), [0, 255, 0, 255]);
            assert_eq!(pixel(&bytes, 40, 32), [0; 4]);
        }
        let mut t = target(&g, 1, false);
        let bytes = pixels(&g, &mut t, |f| {
            renderer
                .draw(
                    &mut f.render_pass().begin().unwrap(),
                    &path,
                    PathDraw::new([1.; 4]).transform(
                        Transform2D::scale(8., 8.).then(Transform2D::translation(10.25, 10.)),
                    ),
                )
                .unwrap()
        });
        assert_eq!(pixel(&bytes, 9, 14), [0; 4]);
        assert!((i16::from(pixel(&bytes, 10, 14)[3]) - 191).abs() <= 2);
        assert_eq!(pixel(&bytes, 12, 14), [255; 4]);
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn stencil_policy_and_read_only_validation_apply_to_paths() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let stencil = |compare, op, write_mask| wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth24PlusStencil8,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: wgpu::StencilState {
                front: wgpu::StencilFaceState {
                    compare,
                    pass_op: op,
                    ..Default::default()
                },
                back: wgpu::StencilFaceState {
                    compare,
                    pass_op: op,
                    ..Default::default()
                },
                read_mask: 0xff,
                write_mask,
            },
            bias: Default::default(),
        };
        let mut writer = PathRenderer::with_options(
            &g,
            PipelineOptions::new()
                .write_mask(wgpu::ColorWrites::empty())
                .depth_stencil(Some(stencil(
                    wgpu::CompareFunction::Always,
                    wgpu::StencilOperation::Replace,
                    0xff,
                ))),
        );
        let mut reader = PathRenderer::with_options(
            &g,
            PipelineOptions::new().depth_stencil(Some(stencil(
                wgpu::CompareFunction::Equal,
                wgpu::StencilOperation::Keep,
                0,
            ))),
        );
        let mask = writer
            .prepare_path(&box_path(8., 8., 16.), PathOptions::new())
            .unwrap();
        let full = reader
            .prepare_path(&box_path(0., 0., 64.), PathOptions::new())
            .unwrap();
        let mut t = target(&g, 1, true);
        writer.prepare(&t.render_format()).unwrap();
        reader.prepare(&t.render_format()).unwrap();
        let bytes = pixels(&g, &mut t, |f| {
            {
                let mut p = f.render_pass().stencil_reference(1).begin().unwrap();
                writer.draw(&mut p, &mask, hard([1.; 4])).unwrap();
                p.set_stencil_reference(2);
                writer
                    .draw(
                        &mut p,
                        &mask,
                        hard([1.; 4]).transform(Transform2D::translation(24., 24.)),
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
            assert!(matches!(
                writer.draw(&mut p, &mask, PathDraw::default()),
                Err(Error::ReadOnlyStencil)
            ));
            p.set_stencil_reference(1);
            reader.draw(&mut p, &full, hard([1., 0., 0., 1.])).unwrap();
            p.set_stencil_reference(2);
            reader.draw(&mut p, &full, hard([0., 0., 1., 1.])).unwrap();
        });
        assert_eq!(pixel(&bytes, 12, 12), [255, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 40, 40), [0, 0, 255, 255]);
        assert_eq!(pixel(&bytes, 28, 28), [0; 4]);
        let mut missing = target(&g, 1, false);
        let mut f = Frame::for_framebuffer(&mut missing);
        assert!(matches!(
            writer.draw(
                &mut f.render_pass().begin().unwrap(),
                &mask,
                PathDraw::default()
            ),
            Err(Error::DepthStencilMismatch { .. })
        ));
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn open_caps_connected_joins_and_miter_limits_have_expected_pixels() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let mut renderer = PathRenderer::new(&g);
        let mut t = target(&g, 1, false);
        let mut b = Path::builder();
        b.move_to([16., 16.]).line_to([40., 16.]);
        let segment = b.build().unwrap();
        for cap in [LineCap::Butt, LineCap::Square, LineCap::Round] {
            let path = renderer
                .prepare_path(
                    &segment,
                    PathOptions::new().stroke(PathStroke::new(8.).cap(cap)),
                )
                .unwrap();
            let bytes = pixels(&g, &mut t, |f| {
                renderer
                    .draw(&mut f.render_pass().begin().unwrap(), &path, hard([1.; 4]))
                    .unwrap()
            });
            assert_eq!(pixel(&bytes, 20, 16), [255; 4]);
            assert_eq!(
                pixel(&bytes, 13, 16),
                if cap == LineCap::Butt {
                    [0; 4]
                } else {
                    [255; 4]
                }
            );
            assert_eq!(
                pixel(&bytes, 12, 12),
                if cap == LineCap::Square {
                    [255; 4]
                } else {
                    [0; 4]
                }
            );
        }
        let mut b = Path::builder();
        b.move_to([16., 48.])
            .line_to([32., 48.])
            .line_to([32., 32.]);
        let corner = b.build().unwrap();
        for (join, limit, outer, diagonal) in [
            (LineJoin::Miter, 4., true, true),
            (LineJoin::Miter, 1., false, false),
            (LineJoin::Bevel, 4., false, false),
            (LineJoin::Round, 4., false, true),
        ] {
            let path = renderer
                .prepare_path(
                    &corner,
                    PathOptions::new().stroke(PathStroke::new(8.).join(join).miter_limit(limit)),
                )
                .unwrap();
            let bytes = pixels(&g, &mut t, |f| {
                renderer
                    .draw(&mut f.render_pass().begin().unwrap(), &path, hard([1.; 4]))
                    .unwrap()
            });
            assert_eq!(
                pixel(&bytes, 35, 51),
                if outer { [255; 4] } else { [0; 4] },
                "{join:?}, limit={limit}"
            );
            assert_eq!(
                pixel(&bytes, 34, 50),
                if diagonal { [255; 4] } else { [0; 4] },
                "{join:?}, limit={limit}"
            );
        }
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn batches_respect_application_buffer_limits_and_preserve_last_draw() {
    pollster::block_on(async {
        let base = GraphicsContext::headless().await.unwrap();
        let (device, queue) = base
            .adapter()
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: wgpu::Limits {
                    max_buffer_size: 1000,
                    max_uniform_buffer_binding_size: 256,
                    max_storage_buffer_binding_size: 256,
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
        let mut renderer = PathRenderer::new(&g);
        assert!(matches!(
            renderer.prepare_path(
                &box_path(0., 0., 8.),
                PathOptions::new().stroke(PathStroke::new(2.).join(LineJoin::Round))
            ),
            Err(Error::PathTooLarge)
        ));
        let path = renderer
            .prepare_path(&box_path(0., 0., 8.), PathOptions::new())
            .unwrap();
        let mut target = g
            .create_framebuffer(
                FramebufferOptions::new(8, 1)
                    .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC),
            )
            .unwrap();
        renderer.prepare(&target.render_format()).unwrap();
        let mut draws = vec![hard([1., 0., 0., 1.]).transform(Transform2D::scale(1., 0.125)); 1000];
        draws.last_mut().unwrap().color = [0., 0., 1., 1.];
        let mut frame = target.begin_frame().unwrap();
        renderer
            .draw_many(&mut frame.render_pass().begin().unwrap(), &path, &draws)
            .unwrap();
        frame.finish().unwrap();
        assert!(
            target
                .read_rgba8()
                .unwrap()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [0, 0, 255, 255])
        );
        assert!(
            g.upload_pool
                .lock()
                .unwrap()
                .iter()
                .all(|(b, _)| b.size() <= 1000)
        );
        assert!(errors.pop().await.is_none());
    });
}
