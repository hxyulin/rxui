use super::*;
use crate::{
    Framebuffer, FramebufferOptions, Point2D, PointBufferOptions, framebuffer::tests::pixels,
};

fn target(g: &GraphicsContext, samples: u32) -> Framebuffer {
    g.create_framebuffer(
        FramebufferOptions::new(64, 64)
            .sample_count(samples)
            .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC),
    )
    .unwrap()
}
fn pixel(b: &[u8], x: usize, y: usize) -> [u8; 4] {
    b[(y * 64 + x) * 4..(y * 64 + x + 1) * 4]
        .try_into()
        .unwrap()
}
fn read(g: &GraphicsContext, points: &PointBuffer) -> Vec<Option<[f32; 2]>> {
    let b = g.device().create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: points.buffer_bytes(),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut e = g.device().create_command_encoder(&Default::default());
    e.copy_buffer_to_buffer(points.as_wgpu(), 0, &b, 0, points.buffer_bytes());
    let i = g.queue().submit([e.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    b.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    g.device()
        .poll(wgpu::PollType::Wait {
            submission_index: Some(i),
            timeout: Some(std::time::Duration::from_secs(10)),
        })
        .unwrap();
    rx.recv().unwrap().unwrap();
    let values =
        bytemuck::cast_slice::<u8, Point2D>(&b.slice(..).get_mapped_range().unwrap()).to_vec();
    b.unmap();
    (0..points.len())
        .map(|i| values[(points.physical_start() + i) % points.capacity()].position())
        .collect()
}
fn upload(g: &GraphicsContext, values: &[[f32; 2]]) -> PointBuffer {
    let mut p = g
        .create_point_buffer(PointBufferOptions::new(values.len().max(1)))
        .unwrap();
    p.replace(&values.iter().copied().map(Point2D::new).collect::<Vec<_>>())
        .unwrap();
    p
}
fn hard() -> PolylineDraw {
    PolylineDraw::default()
        .width_pixels(4.)
        .antialiasing(EdgeAntialiasing::None)
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn ring_updates_preserve_logical_order_and_failed_updates_are_atomic() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let scope = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        assert!(matches!(
            g.create_point_buffer(PointBufferOptions::new(0)),
            Err(Error::InvalidPointCapacity)
        ));
        assert!(matches!(
            g.create_point_buffer(PointBufferOptions::new(usize::MAX)),
            Err(Error::PointBufferTooLarge)
        ));
        let mut p = g
            .create_point_buffer(PointBufferOptions::new(5).ring())
            .unwrap();
        let sample = |x| Point2D::new([x, 0.]);
        assert_eq!(p.uploaded_bytes(), 0);
        p.append(&[sample(0.), sample(1.), sample(2.), sample(3.)])
            .unwrap();
        assert_eq!(
            p.append(&[sample(4.), sample(5.), sample(6.)]).unwrap(),
            crate::PointAppend {
                written: 3,
                evicted: 2
            }
        );
        assert_eq!(p.physical_start(), 2);
        assert_eq!(
            read(&g, &p),
            vec![
                Some([2., 0.]),
                Some([3., 0.]),
                Some([4., 0.]),
                Some([5., 0.]),
                Some([6., 0.])
            ]
        );
        p.write(2, &[sample(14.), Point2D::gap(), sample(16.)])
            .unwrap();
        assert_eq!(
            read(&g, &p),
            vec![
                Some([2., 0.]),
                Some([3., 0.]),
                Some([14., 0.]),
                None,
                Some([16., 0.])
            ]
        );
        let before = (
            p.uploaded_bytes(),
            p.write_count(),
            p.physical_start(),
            read(&g, &p),
        );
        assert!(matches!(
            p.write(0, &[sample(99.), Point2D::new([f32::NAN, 0.])]),
            Err(Error::InvalidPoint { index: 1 })
        ));
        assert!(matches!(
            p.append(&[sample(99.), Point2D::new([f32::INFINITY, 0.])]),
            Err(Error::InvalidPoint { index: 1 })
        ));
        assert!(matches!(
            p.replace(&[sample(99.), Point2D::new([f32::NAN, f32::NAN])]),
            Err(Error::InvalidPoint { index: 1 })
        ));
        assert!(matches!(
            p.write(4, &[sample(0.), sample(1.)]),
            Err(Error::InvalidPointRange)
        ));
        assert_eq!(
            before,
            (
                p.uploaded_bytes(),
                p.write_count(),
                p.physical_start(),
                read(&g, &p)
            )
        );
        assert_eq!(
            p.append(&(10..18).map(|i| sample(i as f32)).collect::<Vec<_>>())
                .unwrap(),
            crate::PointAppend {
                written: 5,
                evicted: 8
            }
        );
        assert_eq!(
            read(&g, &p),
            (13..18).map(|i| Some([i as f32, 0.])).collect::<Vec<_>>()
        );
        let writes = p.write_count();
        p.append(&[]).unwrap();
        p.clear();
        assert_eq!(p.write_count(), writes);
        assert!(p.is_empty());
        let mut linear = g.create_point_buffer(PointBufferOptions::new(2)).unwrap();
        linear.append(&[sample(0.), sample(1.)]).unwrap();
        assert!(matches!(
            linear.append(&[sample(2.)]),
            Err(Error::PointCapacityExceeded)
        ));
        assert_eq!(read(&g, &linear), vec![Some([0., 0.]), Some([1., 0.])]);
        assert!(scope.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn straight_lines_caps_gaps_and_marker_shapes_render() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let mut lines = PolylineRenderer::new(&g);
        let mut markers = MarkerRenderer::new(&g);
        let mut t = target(&g, 1);
        let p = upload(&g, &[[8., 16.], [24., 16.], [40., 16.]]);
        lines.prepare(&t.render_format()).unwrap();
        markers.prepare(&t.render_format()).unwrap();
        for cap in [LineCap::Butt, LineCap::Square, LineCap::Round] {
            let b = pixels(&g, &mut t, |f| {
                lines
                    .draw(
                        &mut f.render_pass().begin().unwrap(),
                        &p,
                        hard().width_pixels(8.).cap(cap),
                    )
                    .unwrap()
            });
            assert_eq!(pixel(&b, 16, 16), [255; 4]);
            assert_eq!(pixel(&b, 24, 16), [255; 4]);
            assert_eq!(
                pixel(&b, 5, 16),
                if cap == LineCap::Butt {
                    [0; 4]
                } else {
                    [255; 4]
                }
            );
            assert_eq!(
                pixel(&b, 5, 12),
                if cap == LineCap::Square {
                    [255; 4]
                } else {
                    [0; 4]
                }
            );
        }
        let mut gaps = g.create_point_buffer(PointBufferOptions::new(5)).unwrap();
        gaps.replace(&[
            Point2D::new([8., 16.]),
            Point2D::new([16., 16.]),
            Point2D::gap(),
            Point2D::new([32., 16.]),
            Point2D::new([40., 16.]),
        ])
        .unwrap();
        let b = pixels(&g, &mut t, |f| {
            let mut pass = f.render_pass().begin().unwrap();
            lines.draw(&mut pass, &gaps, hard()).unwrap();
            markers
                .draw(
                    &mut pass,
                    &gaps,
                    MarkerDraw::new([0., 1., 0., 1.])
                        .radius_pixels(3.)
                        .antialiasing(EdgeAntialiasing::None)
                        .transform(Transform2D::translation(0., 16.)),
                )
                .unwrap();
        });
        assert_eq!(pixel(&b, 12, 16), [255; 4]);
        assert_eq!(pixel(&b, 24, 16), [0; 4]);
        assert_eq!(pixel(&b, 36, 16), [255; 4]);
        assert_eq!(pixel(&b, 8, 32), [0, 255, 0, 255]);
        assert_eq!(pixel(&b, 24, 32), [0; 4]);
        let p = upload(&g, &[[32., 32.]]);
        for shape in [
            MarkerShape::Circle,
            MarkerShape::Square,
            MarkerShape::Diamond,
        ] {
            let b = pixels(&g, &mut t, |f| {
                markers
                    .draw(
                        &mut f.render_pass().begin().unwrap(),
                        &p,
                        MarkerDraw::default()
                            .radius_pixels(8.)
                            .shape(shape)
                            .antialiasing(EdgeAntialiasing::None),
                    )
                    .unwrap()
            });
            assert_eq!(pixel(&b, 32, 32), [255; 4]);
            assert_eq!(
                pixel(&b, 39, 39),
                if shape == MarkerShape::Square {
                    [255; 4]
                } else {
                    [0; 4]
                }
            );
            assert_eq!(
                pixel(&b, 37, 35),
                if shape == MarkerShape::Diamond {
                    [0; 4]
                } else {
                    [255; 4]
                }
            );
        }
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn connected_joins_do_not_double_blend_and_miter_limit_bevels() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let mut r = PolylineRenderer::new(&g);
        let mut t = target(&g, 1);
        let p = upload(&g, &[[8., 40.], [32., 40.], [32., 16.]]);
        for (join, limit, outer, arc) in [
            (LineJoin::Miter, 4., true, true),
            (LineJoin::Miter, 1., false, false),
            (LineJoin::Bevel, 4., false, false),
            (LineJoin::Round, 4., false, true),
        ] {
            let bytes = pixels(&g, &mut t, |f| {
                r.draw(
                    &mut f.render_pass().begin().unwrap(),
                    &p,
                    hard()
                        .width_pixels(8.)
                        .join(join)
                        .miter_limit(limit)
                        .color([1., 0., 0., 0.5]),
                )
                .unwrap()
            });
            assert!(
                bytes.as_chunks::<4>().0.iter().all(|p| p[3] <= 128),
                "overlapping ordinary join {join:?}"
            );
            assert_eq!(pixel(&bytes, 20, 40), [128, 0, 0, 128]);
            assert_eq!(pixel(&bytes, 29, 37), [128, 0, 0, 128]);
            assert_eq!(
                pixel(&bytes, 35, 43),
                if outer { [128, 0, 0, 128] } else { [0; 4] },
                "{join:?} {limit}"
            );
            assert_eq!(
                pixel(&bytes, 34, 42),
                if arc { [128, 0, 0, 128] } else { [0; 4] },
                "{join:?} {limit}"
            );
        }
        // Duplicate samples and exact reversals stay finite and can intentionally overlap.
        let p = upload(
            &g,
            &[[8., 24.], [8., 24.], [40., 24.], [8., 24.], [8., 24.]],
        );
        for join in [LineJoin::Miter, LineJoin::Bevel, LineJoin::Round] {
            let bytes = pixels(&g, &mut t, |f| {
                r.draw(
                    &mut f.render_pass().begin().unwrap(),
                    &p,
                    hard().join(join).cap(LineCap::Round),
                )
                .unwrap()
            });
            assert_eq!(pixel(&bytes, 24, 24), [255; 4]);
        }
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn ranges_ring_order_widths_and_marker_radii_are_fixed_under_zoom() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let mut r = PolylineRenderer::new(&g);
        let mut m = MarkerRenderer::new(&g);
        let mut ring = g
            .create_point_buffer(PointBufferOptions::new(5).ring())
            .unwrap();
        ring.append(
            &(0..8)
                .map(|i| Point2D::new([i as f32 * 4., 8.]))
                .collect::<Vec<_>>(),
        )
        .unwrap();
        ring.append(&[Point2D::new([32., 8.]), Point2D::new([36., 8.])])
            .unwrap();
        let linear = upload(&g, &[[20., 8.], [24., 8.], [28., 8.], [32., 8.], [36., 8.]]);
        let mut t = target(&g, 1);
        let draw = hard().range(1..4).transform(Transform2D::scale(1., 3.));
        let a = pixels(&g, &mut t, |f| {
            r.draw(&mut f.render_pass().begin().unwrap(), &ring, draw.clone())
                .unwrap()
        });
        let b = pixels(&g, &mut t, |f| {
            r.draw(&mut f.render_pass().begin().unwrap(), &linear, draw.clone())
                .unwrap()
        });
        assert_eq!(a, b);
        assert_eq!(pixel(&a, 28, 24), [255; 4]);
        assert_eq!(pixel(&a, 28, 27), [0; 4]);
        assert_eq!(pixel(&a, 20, 24), [0; 4]);
        let p = upload(&g, &[[8., 8.]]);
        let bytes = pixels(&g, &mut t, |f| {
            m.draw(
                &mut f.render_pass().begin().unwrap(),
                &p,
                MarkerDraw::default()
                    .radius_pixels(3.)
                    .shape(MarkerShape::Square)
                    .transform(Transform2D::scale(3., 3.))
                    .antialiasing(EdgeAntialiasing::None),
            )
            .unwrap()
        });
        assert_eq!(pixel(&bytes, 26, 24), [255; 4]);
        assert_eq!(pixel(&bytes, 28, 24), [0; 4]);
        for samples in [1, 4] {
            if !t.supported_sample_counts().contains(&samples) {
                continue;
            }
            let mut t = target(&g, samples);
            let p = upload(&g, &[[0., 0.5], [1., 0.5]]);
            let bytes = pixels(&g, &mut t, |f| {
                let mut pass = f.render_pass().begin().unwrap();
                pass.set_viewport(8., 8., 32., 32., 0., 1.).unwrap();
                pass.set_scissor_rect(10, 8, 28, 32).unwrap();
                r.draw(
                    &mut pass,
                    &p,
                    hard().space(DrawSpace::Normalized).transform(
                        Transform2D::scale(-1., 1.).then(Transform2D::translation(1., 0.)),
                    ),
                )
                .unwrap();
            });
            assert_eq!(pixel(&bytes, 9, 24), [0; 4]);
            assert_eq!(pixel(&bytes, 16, 24), [255; 4]);
            assert_eq!(pixel(&bytes, 38, 24), [0; 4]);
            assert_eq!(pixel(&bytes, 16, 27), [0; 4]);
        }
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn scopes_painter_and_invalid_draws_restore_state_without_point_uploads() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let mut r = PolylineRenderer::new(&g);
        let mut m = MarkerRenderer::new(&g);
        let mut painter = crate::Painter::new(&g);
        let mut t = target(&g, 1);
        let p = upload(&g, &[[8., 16.], [24., 16.], [40., 16.]]);
        let writes = p.write_count();
        let uploaded = p.uploaded_bytes();
        painter.prepare_points(&t.render_format()).unwrap();
        let transform = Transform2D::scale(0.5, 0.5).then(Transform2D::translation(8., 8.));
        let direct = pixels(&g, &mut t, |f| {
            let mut pass = f.render_pass().begin().unwrap();
            r.draw(&mut pass, &p, hard().transform(transform)).unwrap();
            m.draw(
                &mut pass,
                &p,
                MarkerDraw::new([0., 1., 0., 1.]).transform(transform),
            )
            .unwrap();
        });
        let painted = pixels(&g, &mut t, |f| {
            let mut pass = f.render_pass().begin().unwrap();
            let mut paint = painter.begin(&mut pass).unwrap();
            let mut child = paint.transformed(transform).unwrap();
            child.draw_polyline(&p, hard()).unwrap();
            child
                .draw_markers(&p, MarkerDraw::new([0., 1., 0., 1.]))
                .unwrap();
        });
        assert_eq!(direct, painted);
        let bytes = pixels(&g, &mut t, |f| {
            let mut pass = f.render_pass().begin().unwrap();
            let mut scope = r.bind(&mut pass, &p).unwrap();
            scope.draw(hard()).unwrap();
            m.draw(
                scope.pass(),
                &p,
                MarkerDraw::new([0., 1., 0., 1.]).transform(Transform2D::translation(0., 16.)),
            )
            .unwrap();
            scope.pass().as_wgpu().set_scissor_rect(0, 0, 1, 1);
            scope
                .draw(hard().transform(Transform2D::translation(0., 32.)))
                .unwrap();
            assert!(matches!(
                scope.draw(hard().range(1..99)),
                Err(Error::InvalidPointRange)
            ));
            assert!(matches!(
                scope.draw(hard().color([f32::NAN; 4])),
                Err(Error::InvalidPointDraw)
            ));
            assert!(matches!(
                scope.draw(hard().miter_limit(0.5)),
                Err(Error::InvalidPointDraw)
            ));
            assert!(matches!(
                scope.draw(hard().width_pixels(f32::MAX)),
                Err(Error::InvalidPointDraw)
            ));
        });
        assert_eq!(pixel(&bytes, 20, 48), [255; 4]);
        assert_eq!(pixel(&bytes, 8, 32), [0, 255, 0, 255]);
        assert_eq!((writes, uploaded), (p.write_count(), p.uploaded_bytes()));
        let foreign = GraphicsContext::headless().await.unwrap();
        let other = upload(&foreign, &[[0., 0.], [1., 1.]]);
        let bytes = pixels(&g, &mut t, |f| {
            let mut pass = f.render_pass().begin().unwrap();
            assert!(matches!(
                r.draw(&mut pass, &other, hard()),
                Err(Error::DeviceMismatch)
            ));
            assert!(matches!(
                m.draw(&mut pass, &p, MarkerDraw::default().radius_pixels(-1.)),
                Err(Error::InvalidPointDraw)
            ));
        });
        assert!(bytes.iter().all(|b| *b == 0));
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn warm_recording_reuses_resources_and_updates_have_queue_not_snapshot_semantics() {
    pollster::block_on(async {
        let g = GraphicsContext::headless().await.unwrap();
        let mut r = PolylineRenderer::new(&g);
        let mut m = MarkerRenderer::new(&g);
        let mut t = target(&g, 1);
        let points = upload(&g, &[[8., 16.], [40., 16.]]);
        let buffer = points.as_wgpu().clone();
        let group = points.group.clone();
        for _ in 0..4 {
            pixels(&g, &mut t, |f| {
                let mut p = f.render_pass().begin().unwrap();
                r.draw(&mut p, &points, hard()).unwrap();
                m.draw(&mut p, &points, MarkerDraw::default()).unwrap();
            });
            assert_eq!(points.as_wgpu(), &buffer);
            assert_eq!(points.group, group);
            assert_eq!(r.inner.pipelines.len(), 1);
            assert_eq!(m.inner.pipelines.len(), 1);
            assert_eq!(
                g.upload_pool
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|(_, b)| b.len())
                    .sum::<usize>(),
                160
            );
        }
        let pages: Vec<_> = g
            .upload_pool
            .lock()
            .unwrap()
            .iter()
            .map(|(b, _)| b.clone())
            .collect();
        pixels(&g, &mut t, |f| {
            let mut p = f.render_pass().begin().unwrap();
            r.draw(&mut p, &points, hard()).unwrap();
            m.draw(&mut p, &points, MarkerDraw::default()).unwrap();
        });
        assert!(
            g.upload_pool
                .lock()
                .unwrap()
                .iter()
                .all(|(b, _)| pages.contains(b))
        );
        assert_eq!(points.uploaded_bytes(), 16);
        let mut points = upload(&g, &[[8., 8.]]);
        let bytes = pixels(&g, &mut t, |f| {
            let mut pass = f.render_pass().begin().unwrap();
            m.draw(&mut pass, &points, MarkerDraw::new([1., 0., 0., 1.]))
                .unwrap();
            points.write(0, &[Point2D::new([32., 32.])]).unwrap();
            m.draw(&mut pass, &points, MarkerDraw::new([0., 0., 1., 1.]))
                .unwrap();
            drop(points); // Recorded handles remain valid without an RXUI drawing resource lease.
        });
        assert_eq!(pixel(&bytes, 8, 8), [0; 4]);
        assert_eq!(pixel(&bytes, 32, 32), [0, 0, 255, 255]);
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn low_device_limits_and_stencil_policy_fail_before_drawing() {
    pollster::block_on(async {
        let base = GraphicsContext::headless().await.unwrap();
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
            g.create_point_buffer(PointBufferOptions::new(2)),
            Err(Error::UnsupportedPointLimits)
        ));
        let mut p = PolylineRenderer::new(&g);
        assert!(matches!(
            p.prepare(&target(&g, 1).render_format()),
            Err(Error::UnsupportedPointLimits)
        ));
        let mut painter = crate::Painter::new(&g);
        painter.prepare(&target(&g, 1).render_format()).unwrap();
        let (device, queue) = base
            .adapter()
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: wgpu::Limits {
                    max_buffer_size: 512,
                    max_storage_buffer_binding_size: 256,
                    max_uniform_buffer_binding_size: 256,
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
            g.create_point_buffer(PointBufferOptions::new(33)),
            Err(Error::PointBufferTooLarge)
        ));
        let mut points = g
            .create_point_buffer(PointBufferOptions::new(32).ring())
            .unwrap();
        points.append(&[Point2D::new([1., 1.]); 32]).unwrap();
        let mut markers = MarkerRenderer::new(&g);
        let mut t = g.create_framebuffer(FramebufferOptions::new(8, 1)).unwrap();
        let mut f = t.begin_frame().unwrap();
        markers
            .draw(
                &mut f.render_pass().begin().unwrap(),
                &points,
                MarkerDraw::default(),
            )
            .unwrap();
        f.finish().unwrap();
        let g = base;
        let errors = g.device().push_error_scope(wgpu::ErrorFilter::Validation);
        let state = |compare, op, mask| wgpu::DepthStencilState {
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
        let mut writer = MarkerRenderer::with_options(
            &g,
            PipelineOptions::new()
                .write_mask(wgpu::ColorWrites::empty())
                .depth_stencil(Some(state(
                    wgpu::CompareFunction::Always,
                    wgpu::StencilOperation::Replace,
                    0xff,
                ))),
        );
        let mut reader = PolylineRenderer::with_options(
            &g,
            PipelineOptions::new().depth_stencil(Some(state(
                wgpu::CompareFunction::Equal,
                wgpu::StencilOperation::Keep,
                0,
            ))),
        );
        let point = upload(&g, &[[32., 32.]]);
        let line = upload(&g, &[[0., 32.], [64., 32.]]);
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
                writer
                    .draw(
                        &mut p,
                        &point,
                        MarkerDraw::default()
                            .radius_pixels(8.)
                            .shape(MarkerShape::Diamond),
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
                writer.draw(&mut p, &point, MarkerDraw::default()),
                Err(Error::ReadOnlyStencil)
            ));
            reader.draw(&mut p, &line, hard()).unwrap();
        });
        assert_eq!(pixel(&bytes, 32, 32), [255; 4]);
        assert_eq!(pixel(&bytes, 38, 33), [255; 4]);
        assert_eq!(pixel(&bytes, 40, 33), [0; 4]);
        assert!(errors.pop().await.is_none());
    });
}
