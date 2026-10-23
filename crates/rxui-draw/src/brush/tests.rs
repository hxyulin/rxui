use super::*;
use crate::{
    EdgeAntialiasing, Framebuffer, FramebufferOptions, LineDraw, LineRenderer, Painter, Path,
    PathDraw, PathOptions, PathRenderer, Rect, ShapeDraw, ShapeRenderer,
    framebuffer::tests::pixels,
};

const RED: [f32; 4] = [1., 0., 0., 1.];
const BLUE: [f32; 4] = [0., 0., 1., 1.];
const WHITE: [f32; 4] = [1.; 4];
fn stops() -> [GradientStop; 2] {
    [GradientStop::new(0., RED), GradientStop::new(1., BLUE)]
}
fn target(g: &GraphicsContext, samples: u32) -> Framebuffer {
    g.create_framebuffer(
        FramebufferOptions::new(64, 64)
            .sample_count(samples)
            .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC),
    )
    .unwrap()
}
fn pixel(bytes: &[u8], x: usize, y: usize) -> [u8; 4] {
    bytes[(y * 64 + x) * 4..(y * 64 + x + 1) * 4]
        .try_into()
        .unwrap()
}
fn near(actual: [u8; 4], expected: [u8; 4]) {
    assert!(
        actual
            .into_iter()
            .zip(expected)
            .all(|(a, b)| a.abs_diff(b) <= 2),
        "{actual:?} != {expected:?}"
    );
}
fn rect(x: f32, y: f32, w: f32, h: f32) -> ShapeDraw {
    ShapeDraw::rect(Rect::new(x, y, w, h), WHITE).antialiasing(EdgeAntialiasing::None)
}
fn tint(mut draw: ShapeDraw, color: [f32; 4]) -> ShapeDraw {
    draw.color = color;
    draw
}
fn path(renderer: &mut PathRenderer) -> crate::PreparedPath {
    let mut b = Path::builder();
    b.move_to([0., 0.])
        .line_to([16., 0.])
        .line_to([16., 16.])
        .line_to([0., 16.])
        .close();
    renderer
        .prepare_path(&b.build().unwrap(), PathOptions::new())
        .unwrap()
}

#[test]
fn descriptions_validate_stops_geometry_and_inverse_mapping() {
    let stops = stops();
    for options in [
        BrushOptions::linear([0., 0.], [0., 0.], &stops),
        BrushOptions::linear([0., 0.], [1., 0.], &[]),
        BrushOptions::linear([f32::NAN, 0.], [1., 0.], &stops),
        BrushOptions::linear([0., 0.], [1., 0.], &stops).transform(Transform2D::scale(0., 1.)),
        BrushOptions::radial([0., 0.], 0., &stops),
        BrushOptions::radial([0., 0.], f32::INFINITY, &stops),
        BrushOptions::solid([1., 1., 1., 2.]),
    ] {
        assert!(matches!(encode(options), Err(Error::InvalidBrush)));
    }
    for invalid in [
        [GradientStop::new(0.6, RED), GradientStop::new(0.4, BLUE)],
        [GradientStop::new(-0.1, RED), GradientStop::new(1., BLUE)],
        [
            GradientStop::new(0., RED),
            GradientStop::new(f32::NAN, BLUE),
        ],
        [
            GradientStop::new(0., RED),
            GradientStop::new(1., [f32::NAN, 0., 0., 1.]),
        ],
    ] {
        assert!(matches!(
            encode(BrushOptions::linear([0., 0.], [1., 0.], &invalid)),
            Err(Error::InvalidBrush)
        ));
    }
    let (uniform, _) = encode(
        BrushOptions::radial([1., 2.], 2., &stops)
            .transform(Transform2D::scale(2., 4.).then(Transform2D::translation(10., 20.))),
    )
    .unwrap();
    assert_eq!(uniform.axes, [0.25, 0., 0., 0.125]);
    assert_eq!(uniform.translation, [-3., -3.5, 0., 0.]);
    let (_, premul) = encode(BrushOptions::solid([0., 1., 1., 0.])).unwrap();
    assert_eq!(premul[0].color, [0.; 4]);
    assert!(
        encode(BrushOptions::linear(
            [0., 0.],
            [1., 0.],
            &[GradientStop::new(0.5, RED)]
        ))
        .is_ok()
    );
    assert!(
        encode(BrushOptions::linear(
            [0., 0.],
            [1., 0.],
            &[GradientStop::new(0.5, RED), GradientStop::new(0.5, BLUE)]
        ))
        .is_ok()
    );
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn gradients_render_exact_stops_hard_edges_and_premultiplied_tints() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut shapes = ShapeRenderer::new(&g);
        let mut paths = PathRenderer::new(&g);
        let path = path(&mut paths);
        let linear = g
            .create_brush(BrushOptions::linear([0., 0.], [16., 0.], &stops()))
            .unwrap();
        let hard = g
            .create_brush(BrushOptions::linear(
                [0., 0.],
                [17., 0.],
                &[
                    GradientStop::new(0., RED),
                    GradientStop::new(0.5, RED),
                    GradientStop::new(0.5, BLUE),
                    GradientStop::new(1., BLUE),
                ],
            ))
            .unwrap();
        let transparent = g
            .create_brush(BrushOptions::linear(
                [0., 0.],
                [16., 0.],
                &[
                    GradientStop::new(0., RED),
                    GradientStop::new(1., [0., 0., 1., 0.]),
                ],
            ))
            .unwrap();
        let mut t = target(&g, 1);
        shapes.prepare_brush(&t.render_format()).unwrap();
        paths.prepare_brush(&t.render_format()).unwrap();
        let bytes = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            paths
                .draw_with_brush(
                    &mut p,
                    &path,
                    &linear,
                    PathDraw::default().antialiasing(EdgeAntialiasing::None),
                )
                .unwrap();
            shapes
                .draw_with_brush(
                    &mut p,
                    &hard,
                    rect(0., 0., 20., 8.).transform(Transform2D::translation(0., 20.)),
                )
                .unwrap();
            shapes
                .draw_with_brush(
                    &mut p,
                    &transparent,
                    tint(rect(0., 0., 16., 8.), [0.5, 1., 1., 0.5])
                        .transform(Transform2D::translation(0., 32.)),
                )
                .unwrap();
        });
        near(pixel(&bytes, 7, 7), [135, 0, 120, 255]);
        assert_eq!(pixel(&bytes, 7, 23), [255, 0, 0, 255]);
        assert_eq!(pixel(&bytes, 8, 23), [0, 0, 255, 255]); // Exact duplicated stop is right-continuous.
        near(pixel(&bytes, 7, 35), [34, 0, 0, 68]); // Hidden blue in the transparent stop never bleeds.
        assert_eq!(linear.stop_count(), 2);
        assert_eq!(linear.buffer_bytes(), 112);
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn spread_and_radial_geometry_follow_brush_and_draw_transforms() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let mut shapes = ShapeRenderer::new(&g);
        let mut lines = LineRenderer::new(&g);
        let mut t = target(&g, 1);
        for spread in [
            GradientSpread::Clamp,
            GradientSpread::Repeat,
            GradientSpread::Reflect,
        ] {
            let brush = g
                .create_brush(BrushOptions::linear([8., 0.], [16., 0.], &stops()).spread(spread))
                .unwrap();
            let bytes = pixels(&g, &mut t, |f| {
                let mut p = f.render_pass().begin().unwrap();
                shapes
                    .draw_with_brush(&mut p, &brush, rect(0., 0., 32., 8.))
                    .unwrap();
                lines
                    .draw_with_brush(
                        &mut p,
                        &brush,
                        LineDraw::new([0., 16.], [32., 16.], WHITE)
                            .width(8.)
                            .antialiasing(EdgeAntialiasing::None),
                    )
                    .unwrap();
            });
            for x in [0, 7, 8, 12, 15, 16, 20, 24, 31] {
                let v = (x as f32 + 0.5 - 8.) / 8.;
                let u = match spread {
                    GradientSpread::Clamp => v.clamp(0., 1.),
                    GradientSpread::Repeat => v - v.floor(),
                    GradientSpread::Reflect => 1. - ((v * 0.5 - (v * 0.5).floor()) * 2. - 1.).abs(),
                };
                let expected = [
                    ((1. - u) * 255.).round() as u8,
                    0,
                    (u * 255.).round() as u8,
                    255,
                ];
                near(pixel(&bytes, x, 4), expected);
                near(pixel(&bytes, x, 16), expected);
            }
        }
        let radial = g
            .create_brush(
                BrushOptions::radial([0., 0.], 8., &stops()).transform(
                    Transform2D::scale(2., 1.).then(Transform2D::translation(16.5, 16.5)),
                ),
            )
            .unwrap();
        let bytes = pixels(&g, &mut t, |f| {
            shapes
                .draw_with_brush(
                    &mut f.render_pass().begin().unwrap(),
                    &radial,
                    rect(0., 0., 40., 40.),
                )
                .unwrap()
        });
        assert_eq!(pixel(&bytes, 16, 16), [255, 0, 0, 255]);
        near(pixel(&bytes, 24, 16), [128, 0, 128, 255]);
        near(pixel(&bytes, 16, 20), [128, 0, 128, 255]);
        assert_eq!(pixel(&bytes, 16, 24), [0, 0, 255, 255]);
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn batches_painter_and_scopes_preserve_pixels_order_and_bindings() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let brush = g
            .create_brush(BrushOptions::linear([0., 0.], [32., 0.], &stops()))
            .unwrap();
        let mut shapes = ShapeRenderer::new(&g);
        let mut lines = LineRenderer::new(&g);
        let mut paths = PathRenderer::new(&g);
        let path = path(&mut paths);
        let shape_draws: Vec<_> = (0..1200)
            .map(|i| {
                tint(
                    rect((i % 32) as f32, (i % 17) as f32, 16., 16.),
                    [1., 1., 1., 0.03],
                )
            })
            .collect();
        let line_draws: Vec<_> = (0..1200)
            .map(|i| {
                LineDraw::new(
                    [0., (i % 48) as f32],
                    [32., (i % 48) as f32],
                    [1., 1., 1., 0.03],
                )
                .width(2.)
            })
            .collect();
        let path_draws: Vec<_> = (0..2050)
            .map(|i| {
                PathDraw::new([1., 1., 1., 0.03])
                    .transform(Transform2D::translation((i % 32) as f32, (i % 17) as f32))
            })
            .collect();
        let mut t = target(&g, 1);
        let individual = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            for d in &shape_draws {
                shapes.draw_with_brush(&mut p, &brush, *d).unwrap();
            }
            for d in &line_draws {
                lines.draw_with_brush(&mut p, &brush, *d).unwrap();
            }
            for d in &path_draws {
                paths.draw_with_brush(&mut p, &path, &brush, *d).unwrap();
            }
        });
        let batch = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            shapes
                .draw_many_with_brush(&mut p, &brush, &shape_draws)
                .unwrap();
            lines
                .draw_many_with_brush(&mut p, &brush, &line_draws)
                .unwrap();
            paths
                .draw_many_with_brush(&mut p, &path, &brush, &path_draws)
                .unwrap();
        });
        assert_eq!(individual, batch);
        let mut painter = Painter::new(&g);
        painter.prepare_brush(&t.render_format()).unwrap();
        let transform = Transform2D::scale(0.5, 0.5).then(Transform2D::translation(4., 8.));
        let painted = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            let mut paint = painter.begin(&mut p).unwrap();
            let mut child = paint.transformed(transform).unwrap();
            child.draw_shapes_with_brush(&brush, &shape_draws).unwrap();
            child.draw_lines_with_brush(&brush, &line_draws).unwrap();
            child
                .draw_paths_with_brush(&path, &brush, &path_draws)
                .unwrap();
        });
        let direct = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            for d in &shape_draws {
                shapes
                    .draw_with_brush(&mut p, &brush, d.transform(d.transform.then(transform)))
                    .unwrap();
            }
            for d in &line_draws {
                lines
                    .draw_with_brush(&mut p, &brush, d.transform(d.transform.then(transform)))
                    .unwrap();
            }
            for d in &path_draws {
                paths
                    .draw_with_brush(
                        &mut p,
                        &path,
                        &brush,
                        d.transform(d.transform.then(transform)),
                    )
                    .unwrap();
            }
        });
        assert_eq!(painted, direct);
        let solid = g.create_brush(BrushOptions::solid(BLUE)).unwrap();
        let bytes = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            let mut scope = paths.bind_with_brush(&mut p, &brush).unwrap();
            scope.draw(&path, PathDraw::default()).unwrap();
            shapes
                .draw_with_brush(scope.pass(), &solid, rect(20., 0., 16., 16.))
                .unwrap();
            scope.pass().as_wgpu().set_scissor_rect(0, 0, 1, 1);
            scope
                .draw(
                    &path,
                    PathDraw::default().transform(Transform2D::translation(40., 0.)),
                )
                .unwrap();
        });
        assert_eq!(pixel(&bytes, 24, 8), [0, 0, 255, 255]);
        near(pixel(&bytes, 48, 8), pixel(&bytes, 8, 8));
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn invalid_batches_devices_and_overflow_record_no_draws() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let foreign = GraphicsContext::headless().await.unwrap();
        let brush = g
            .create_brush(BrushOptions::linear([0., 0.], [32., 0.], &stops()))
            .unwrap();
        let other = foreign.create_brush(BrushOptions::solid(RED)).unwrap();
        let tiny = g
            .create_brush(BrushOptions::linear([0., 0.], [1e-30, 0.], &stops()))
            .unwrap();
        let hdr = g
            .create_brush(BrushOptions::solid([f32::MAX, 0., 0., 1.]))
            .unwrap();
        let mut shapes = ShapeRenderer::new(&g);
        let mut lines = LineRenderer::new(&g);
        let mut paths = PathRenderer::new(&g);
        let path = path(&mut paths);
        let mut t = target(&g, 1);
        let bytes = pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            assert!(matches!(
                shapes.draw_many_with_brush(
                    &mut p,
                    &brush,
                    &[
                        rect(0., 0., 8., 8.),
                        tint(rect(8., 0., 8., 8.), [f32::NAN; 4])
                    ]
                ),
                Err(Error::InvalidShapeDraw)
            ));
            assert!(matches!(
                lines.draw_many_with_brush(
                    &mut p,
                    &brush,
                    &[
                        LineDraw::new([0., 0.], [8., 0.], WHITE),
                        LineDraw::new([0., 0.], [f32::NAN, 0.], WHITE)
                    ]
                ),
                Err(Error::InvalidLineDraw)
            ));
            assert!(matches!(
                paths.draw_many_with_brush(
                    &mut p,
                    &path,
                    &brush,
                    &[PathDraw::default(), PathDraw::new([f32::NAN; 4])]
                ),
                Err(Error::InvalidPathDraw)
            ));
            assert!(matches!(
                shapes.draw_with_brush(&mut p, &other, rect(0., 0., 8., 8.)),
                Err(Error::DeviceMismatch)
            ));
            assert!(matches!(
                paths.draw_with_brush(&mut p, &path, &other, PathDraw::default()),
                Err(Error::DeviceMismatch)
            ));
            assert!(matches!(
                shapes.draw_with_brush(&mut p, &tiny, rect(1e20, 0., 8., 8.)),
                Err(Error::InvalidBrushDraw)
            ));
            assert!(matches!(
                paths.draw_with_brush(&mut p, &path, &hdr, PathDraw::default()),
                Err(Error::InvalidBrushDraw)
            ));
            // A smaller tint is valid even when a white tint would overflow.
            paths
                .draw_with_brush(&mut p, &path, &hdr, PathDraw::new([0., 0., 0., 0.]))
                .unwrap();
        });
        assert!(bytes.iter().all(|b| *b == 0));
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn msaa_viewport_reflection_and_normalized_brush_coordinates_are_consistent() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let brush = g
            .create_brush(BrushOptions::linear([0., 0.], [1., 0.], &stops()))
            .unwrap();
        let mut shapes = ShapeRenderer::new(&g);
        let mut paths = PathRenderer::new(&g);
        let mut b = Path::builder();
        b.move_to([0., 0.])
            .line_to([1., 0.])
            .line_to([1., 1.])
            .line_to([0., 1.])
            .close();
        let path = paths
            .prepare_path(&b.build().unwrap(), PathOptions::new())
            .unwrap();
        for samples in [1, 4] {
            if !target(&g, 1).supported_sample_counts().contains(&samples) {
                continue;
            }
            let mut t = target(&g, samples);
            let shape = pixels(&g, &mut t, |f| {
                let mut p = f.render_pass().begin().unwrap();
                p.set_viewport(8., 8., 32., 32., 0., 1.).unwrap();
                p.set_scissor_rect(10, 8, 28, 32).unwrap();
                shapes
                    .draw_with_brush(
                        &mut p,
                        &brush,
                        rect(0., 0., 1., 1.)
                            .space(crate::DrawSpace::Normalized)
                            .transform(
                                Transform2D::scale(-1., 1.).then(Transform2D::translation(1., 0.)),
                            ),
                    )
                    .unwrap();
            });
            let retained = pixels(&g, &mut t, |f| {
                let mut p = f.render_pass().begin().unwrap();
                p.set_viewport(8., 8., 32., 32., 0., 1.).unwrap();
                p.set_scissor_rect(10, 8, 28, 32).unwrap();
                paths
                    .draw_with_brush(
                        &mut p,
                        &path,
                        &brush,
                        PathDraw::default()
                            .space(crate::DrawSpace::Normalized)
                            .antialiasing(EdgeAntialiasing::None)
                            .transform(
                                Transform2D::scale(-1., 1.).then(Transform2D::translation(1., 0.)),
                            ),
                    )
                    .unwrap();
            });
            assert_eq!(shape, retained);
            assert_eq!(pixel(&shape, 9, 16), [0; 4]);
            near(pixel(&shape, 16, 16), [68, 0, 187, 255]);
            assert_eq!(pixel(&shape, 38, 16), [0; 4]);
        }
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn brush_storage_and_parameter_pages_are_retained_across_frames_and_owner_drop() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let brush = g
            .create_brush(BrushOptions::linear([0., 0.], [16., 0.], &stops()))
            .unwrap();
        let clone = brush.clone();
        assert!(Arc::ptr_eq(&clone.storage, &brush.storage));
        drop(clone);
        let group = brush.storage.group.clone();
        let uniform = brush.storage.uniform.clone();
        let stops = brush.storage.stops.clone();
        let mut paths = PathRenderer::new(&g);
        let path = path(&mut paths);
        let draws = vec![PathDraw::default(); 1100];
        let mut t = target(&g, 1);
        pixels(&g, &mut t, |f| {
            paths
                .draw_many_with_brush(&mut f.render_pass().begin().unwrap(), &path, &brush, &draws)
                .unwrap()
        });
        let pages: Vec<_> = g
            .upload_pool
            .lock()
            .unwrap()
            .iter()
            .map(|(b, _)| b.clone())
            .collect();
        for _ in 0..3 {
            pixels(&g, &mut t, |f| {
                paths
                    .draw_many_with_brush(
                        &mut f.render_pass().begin().unwrap(),
                        &path,
                        &brush,
                        &draws,
                    )
                    .unwrap()
            });
            assert_eq!(brush.storage.group, group);
            assert_eq!(brush.storage.uniform, uniform);
            assert_eq!(brush.storage.stops, stops);
            let pool = g.upload_pool.lock().unwrap();
            assert!(pool.iter().all(|(b, _)| pages.contains(b)));
            assert_eq!(
                pool.iter().map(|(_, bytes)| bytes.len()).sum::<usize>(),
                draws.len() * 64
            );
        }
        let weak = Arc::downgrade(&brush.storage);
        let bytes = pixels(&g, &mut t, |f| {
            paths
                .draw_with_brush(
                    &mut f.render_pass().begin().unwrap(),
                    &path,
                    &brush,
                    PathDraw::default(),
                )
                .unwrap();
            drop(brush);
            assert!(weak.upgrade().is_none());
        });
        near(pixel(&bytes, 7, 7), [135, 0, 120, 255]);
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn application_limits_reject_unsupported_brushes_and_split_primitive_pages() {
    pollster::block_on(async {
        let base = GraphicsContext::headless().await.unwrap();
        let (device, queue) = base
            .adapter()
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: wgpu::Limits {
                    max_buffer_size: 512,
                    max_uniform_buffer_binding_size: 128,
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
        assert!(matches!(
            g.create_brush(BrushOptions::linear(
                [0., 0.],
                [1., 0.],
                &[GradientStop::new(0., WHITE); 5]
            )),
            Err(Error::BrushTooLarge)
        ));
        let brush = g.create_brush(BrushOptions::solid(BLUE)).unwrap();
        let mut shapes = ShapeRenderer::new(&g);
        let mut target = g
            .create_framebuffer(
                FramebufferOptions::new(8, 1)
                    .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC),
            )
            .unwrap();
        let mut frame = target.begin_frame().unwrap();
        shapes
            .draw_many_with_brush(
                &mut frame.render_pass().begin().unwrap(),
                &brush,
                &vec![rect(0., 0., 8., 1.); 1000],
            )
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
        assert!(errors.pop().await.is_none());
        let (device, queue) = base
            .adapter()
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: wgpu::Limits {
                    max_storage_buffers_per_shader_stage: 0,
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
        assert!(matches!(
            g.create_brush(BrushOptions::solid(WHITE)),
            Err(Error::UnsupportedBrushLimits)
        ));
        let mut shapes = ShapeRenderer::new(&g);
        shapes.prepare(&target.render_format()).unwrap(); // Color-only drawing remains available.
        assert!(matches!(
            shapes.prepare_brush(&target.render_format()),
            Err(Error::UnsupportedBrushLimits)
        ));
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn transparent_brush_fragments_do_not_write_stencil_and_read_only_is_validated() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let stencil = |compare, op, mask| wgpu::DepthStencilState {
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
                write_mask: mask,
            },
            bias: Default::default(),
        };
        let options = crate::PipelineOptions::new()
            .write_mask(wgpu::ColorWrites::empty())
            .depth_stencil(Some(stencil(
                wgpu::CompareFunction::Always,
                wgpu::StencilOperation::Replace,
                0xff,
            )));
        let mut shapes = ShapeRenderer::with_options(&g, options.clone());
        let mut writer = PathRenderer::with_options(&g, options);
        let path = path(&mut writer);
        let mut reader = ShapeRenderer::with_options(
            &g,
            crate::PipelineOptions::new().depth_stencil(Some(stencil(
                wgpu::CompareFunction::Equal,
                wgpu::StencilOperation::Keep,
                0,
            ))),
        );
        let brush = g
            .create_brush(BrushOptions::linear(
                [0., 0.],
                [16., 0.],
                &[
                    GradientStop::new(0., [0.; 4]),
                    GradientStop::new(0.5, [0.; 4]),
                    GradientStop::new(0.5, WHITE),
                    GradientStop::new(1., WHITE),
                ],
            ))
            .unwrap();
        let green = g
            .create_brush(BrushOptions::solid([0., 1., 0., 1.]))
            .unwrap();
        let mut t = g
            .create_framebuffer(
                FramebufferOptions::new(64, 64)
                    .depth_stencil(wgpu::TextureFormat::Depth24PlusStencil8)
                    .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC),
            )
            .unwrap();
        let bytes = pixels(&g, &mut t, |f| {
            {
                let mut p = f.render_pass().stencil_reference(1).begin().unwrap();
                shapes
                    .draw_with_brush(&mut p, &brush, rect(0., 0., 16., 16.))
                    .unwrap();
                p.set_stencil_reference(2);
                writer
                    .draw_with_brush(
                        &mut p,
                        &path,
                        &brush,
                        PathDraw::default()
                            .antialiasing(EdgeAntialiasing::None)
                            .transform(Transform2D::translation(0., 24.)),
                    )
                    .unwrap();
            }
            let mut p = f
                .render_pass()
                .load_all()
                .depth_ops(None)
                .stencil_ops(None)
                .stencil_reference(1)
                .begin()
                .unwrap();
            assert!(matches!(
                writer.draw_with_brush(&mut p, &path, &brush, PathDraw::default()),
                Err(Error::ReadOnlyStencil)
            ));
            reader
                .draw_with_brush(&mut p, &green, rect(0., 0., 64., 64.))
                .unwrap();
            p.set_stencil_reference(2);
            reader
                .draw_with_brush(&mut p, &green, rect(0., 0., 64., 64.))
                .unwrap();
        });
        for y in [8, 32] {
            assert_eq!(pixel(&bytes, 4, y), [0; 4]);
            assert_eq!(pixel(&bytes, 12, y), [0, 255, 0, 255]);
        }
        assert!(errors.pop().await.is_none());
    });
}
