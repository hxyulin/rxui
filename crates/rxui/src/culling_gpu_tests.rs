use super::image_gpu_tests::{Page, pixels, prepare, read_pixel};
use super::*;
use crate::*;
use astrelis::{FramebufferOptions, wgpu};

#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn dock_drag_preview_respects_dpi_viewport_scissor_theme_and_isolated_content() {
    struct DragPage {
        tree: DockTree,
        preview: bool,
    }
    impl View for DragPage {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            stack()
                .size(256., 192.)
                .padding(8.)
                .theme(Theme::dark().colors(|c| c.focus = [0., 1., 0., 1.]))
                .child(
                    dock(&self.tree, |key| {
                        dock_panel(
                            format!("{key:?}"),
                            stack().fill_width().fill_height().background(
                                if key == &Key::from("c") {
                                    [0., 0., 1., 1.]
                                } else {
                                    [1., 0., 0., 1.]
                                },
                            ),
                        )
                    })
                    .size(240., 176.)
                    .min_pane_size(30., 30.)
                    .drop_preview(self.preview)
                    .on_event(cx.listener(|s, e: &DockEvent, _| {
                        let _ = s.tree.apply(e);
                    }))
                    .into_element()
                    .opacity(0.5),
                )
        }
    }
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let errors = graphics
            .device()
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let mut target = graphics
            .create_framebuffer(
                FramebufferOptions::new(512, 384)
                    .format(wgpu::TextureFormat::Rgba8Unorm)
                    .sample_count(4)
                    .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC),
            )
            .unwrap();
        let mut tree = DockTree::from_panels(["a", "b"]).unwrap();
        let main = tree.root().id();
        tree.split(main, DockSide::Right, "c", SplitPosition::default())
            .unwrap();
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| {
            cx.new(|_| DragPage {
                tree,
                preview: true,
            })
        });
        let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
        let mut painter = UiPainter::new(&graphics);
        painter
            .fonts_mut()
            .load_font_shared(Arc::<[u8]>::from(
                include_bytes!("../tests/fonts/SourceSans3-Regular.otf").as_slice(),
            ))
            .unwrap();
        ui.prepare(&mut runtime, [256., 192.], &mut painter)
            .unwrap();
        painter.prepare(&ui, &target.render_format(), 2.).unwrap();
        let source = ui
            .semantics()
            .find(|n| n.role == SemanticRole::Tab && n.label == Some("String(\"a\")"))
            .unwrap()
            .id;
        let destination = ui
            .semantics()
            .find(|n| n.role == SemanticRole::Tab && n.label == Some("String(\"c\")"))
            .unwrap()
            .controls
            .unwrap();
        let b = ui.element(destination).unwrap().bounds;
        let point = [b.x + b.width * 0.5, b.y + b.height * 0.5];
        let at = [
            (12. + point[0] * 2.) as usize,
            (8. + point[1] * 2.) as usize,
        ];
        let mut samples = Vec::new();
        for stage in 0..4 {
            if stage == 1 {
                let b = ui.element(source).unwrap().bounds;
                ui.pointer(
                    &mut runtime,
                    PointerEvent::Pressed([b.x + b.width * 0.5, b.y + b.height * 0.5]),
                )
                .unwrap();
                ui.pointer(&mut runtime, PointerEvent::Moved(point))
                    .unwrap();
                assert!(ui.dock_drag().unwrap().preview.is_some());
            } else if stage == 2 {
                runtime.update(|cx| root.update(cx, |s, _| s.preview = false));
                ui.prepare(&mut runtime, [256., 192.], &mut painter)
                    .unwrap();
                painter.prepare(&ui, &target.render_format(), 2.).unwrap();
                assert!(ui.dock_drag().unwrap().preview.is_some());
            } else if stage == 3 {
                ui.key(
                    &mut runtime,
                    KeyEvent {
                        key: KeyboardKey::Escape,
                        pressed: true,
                        repeat: false,
                        modifiers: Modifiers::default(),
                    },
                )
                .unwrap();
                assert!(ui.dock_drag().is_none());
            }
            let texture = target.color_texture().unwrap().clone();
            let buffer = graphics.device().create_buffer(&wgpu::BufferDescriptor {
                label: Some("Dock preview readback"),
                size: 2048 * 384,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut frame = target.begin_frame().unwrap();
            painter
                .compose(&ui, &mut frame, 2., |frame, composed| {
                    let mut pass = frame
                        .render_pass()
                        .clear_color(wgpu::Color::BLACK)
                        .begin()?;
                    pass.set_viewport(12., 8., 500., 376., 0., 1.)?;
                    pass.set_scissor_rect(32, 28, 464, 328)?;
                    composed.paint(&mut pass)?;
                    assert_eq!(pass.scissor_rect(), [32, 28, 464, 328]);
                    assert_eq!(pass.viewport(), [12., 8., 500., 376., 0., 1.]);
                    Ok(())
                })
                .unwrap();
            frame.encoder().copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: Default::default(),
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(2048),
                        rows_per_image: Some(384),
                    },
                },
                texture.size(),
            );
            let bytes = pixels(&graphics, &buffer, frame.finish().unwrap());
            assert_eq!(
                &bytes[10 * 2048 + 10 * 4..10 * 2048 + 10 * 4 + 4],
                &[0, 0, 0, 255]
            );
            samples.push(bytes[at[1] * 2048 + at[0] * 4..at[1] * 2048 + at[0] * 4 + 4].to_vec());
        }
        assert_eq!(samples[0], samples[2]);
        assert_eq!(samples[0], samples[3]);
        assert!(samples[0][2].abs_diff(128) <= 1);
        assert!(
            samples[1][1].abs_diff(20) <= 2,
            "preview color {:?}",
            samples[1]
        );
        assert!(samples[1][2] < samples[0][2]);
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn dock_tree_reparenting_and_collapse_paint_real_font_panels_in_their_slots() {
    struct DockPage {
        tree: DockTree,
    }
    fn color(key: &Key) -> [f32; 4] {
        if key == &Key::from("editor") {
            [1., 0., 0., 1.]
        } else if key == &Key::from("preview") {
            [0., 1., 0., 1.]
        } else {
            [0., 0., 1., 1.]
        }
    }
    impl View for DockPage {
        fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            dock(&self.tree, |key| {
                dock_panel(
                    format!("{key:?}"),
                    stack()
                        .key(format!("ink {key:?}"))
                        .fill_width()
                        .fill_height()
                        .background(color(key))
                        .child(
                            text_input("Real font text")
                                .absolute()
                                .width(100.)
                                .height(40.),
                        ),
                )
            })
            .size(512., 384.)
            .min_pane_size(60., 60.)
        }
    }
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let errors = graphics
            .device()
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let mut target = graphics
            .create_framebuffer(
                FramebufferOptions::new(512, 384)
                    .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC),
            )
            .unwrap();
        let mut tree = DockTree::from_panels(["editor", "preview"]).unwrap();
        let main = tree.root().id();
        let output = tree
            .split(
                main,
                DockSide::Bottom,
                "output",
                SplitPosition::Fraction(0.6),
            )
            .unwrap();
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| cx.new(|_| DockPage { tree }));
        let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
        let mut painter = UiPainter::new(&graphics);
        painter
            .fonts_mut()
            .load_font_shared(Arc::<[u8]>::from(
                include_bytes!("../tests/fonts/SourceSans3-Regular.otf").as_slice(),
            ))
            .unwrap();
        for stage in 0..4 {
            runtime.update(|cx| {
                root.update(cx, |s, _| match stage {
                    1 => {
                        s.tree
                            .dock_panel(
                                &Key::from("preview"),
                                main,
                                DockSide::Right,
                                SplitPosition::Fraction(0.5),
                            )
                            .unwrap();
                    }
                    2 => {
                        s.tree.move_panel(&Key::from("editor"), output, 0).unwrap();
                    }
                    3 => {
                        for key in ["editor", "preview", "output"] {
                            s.tree.remove(&Key::from(key));
                        }
                    }
                    _ => {}
                })
            });
            ui.prepare(&mut runtime, [512., 384.], &mut painter)
                .unwrap();
            painter.prepare(&ui, &target.render_format(), 1.).unwrap();
            let samples: Vec<_> = ui
                .elements()
                .filter(|e| {
                    e.key
                        .is_some_and(|k| matches!(k,Key::String(s) if s.starts_with("ink ")))
                })
                .map(|e| {
                    assert!(e.bounds.width > 0. && e.bounds.height > 40.);
                    let point = [
                        (e.bounds.x + e.bounds.width * 0.5) as usize,
                        (e.bounds.y + e.bounds.height - 8.) as usize,
                    ];
                    (point, e.background.unwrap())
                })
                .collect();
            assert_eq!(samples.len(), [2, 3, 2, 0][stage]);
            let buffer = graphics.device().create_buffer(&wgpu::BufferDescriptor {
                label: Some("Dock readback"),
                size: 2048 * 384,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let texture = target.color_texture().unwrap().clone();
            let mut frame = target.begin_frame().unwrap();
            painter
                .compose(&ui, &mut frame, 1., |frame, composed| {
                    let mut pass = frame
                        .render_pass()
                        .clear_color(wgpu::Color::BLACK)
                        .begin()?;
                    composed.paint(&mut pass)
                })
                .unwrap();
            frame.encoder().copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: Default::default(),
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(2048),
                        rows_per_image: Some(384),
                    },
                },
                texture.size(),
            );
            let bytes = pixels(&graphics, &buffer, frame.finish().unwrap());
            for ([x, y], expected) in samples {
                let at = &bytes[y * 2048 + x * 4..y * 2048 + x * 4 + 4];
                assert_eq!(
                    at,
                    &expected.map(|c| (c * 255.) as u8),
                    "stage {stage}, point {x},{y}"
                );
            }
        }
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn scroll_culling_retains_glyphs_overflow_and_independent_descendants() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let errors = graphics
            .device()
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let mut target = graphics
            .create_framebuffer(
                FramebufferOptions::new(64, 64)
                    .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC),
            )
            .unwrap();
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| {
            cx.new(|_| {
                Page(
                    stack()
                        .size(64., 64.)
                        .clip()
                        .child(
                            column()
                                .key("viewport")
                                .absolute()
                                .size(32., 64.)
                                .scroll_y()
                                .clip()
                                .child(
                                    column()
                                        .children((0..1000).map(|i| {
                                            label("row").key(i).font_size(8.).height(16.)
                                        })),
                                ),
                        )
                        // An offscreen layout parent must not suppress its visible child.
                        .child(
                            stack()
                                .absolute()
                                .left(96.)
                                .size(8., 16.)
                                .child(label("D").absolute().left(-60.).top(8.).font_size(12.)),
                        )
                        // Ink can extend beyond a label's zero-height layout box.
                        .child(
                            label("Z")
                                .key("overflow")
                                .absolute()
                                .left(48.)
                                .top(8.)
                                .font_size(12.)
                                .height(0.),
                        )
                        .child(
                            column()
                                .absolute()
                                .left(40.25)
                                .top(40.25)
                                .size(8., 8.)
                                .background([0., 1., 0., 1.]),
                        ),
                )
            })
        });
        let mut ui = Ui::new(&mut runtime, root).unwrap();
        let mut painter = UiPainter::new(&graphics);
        painter
            .fonts_mut()
            .load_font_shared(Arc::<[u8]>::from(
                include_bytes!("../tests/fonts/SourceSans3-Regular.otf").as_slice(),
            ))
            .unwrap();
        prepare(&mut runtime, &mut ui, &mut painter, &target.render_format());
        let ui_stats = ui.stats();
        let text_stats = painter.painter().text().stats();
        let overflow = ui
            .elements()
            .find(|e| e.key == Some(&Key::from("overflow")))
            .unwrap();
        assert_eq!(overflow.bounds.height, 0.);
        let texture = target.color_texture().unwrap().clone();
        for scrolled in [false, true] {
            if scrolled {
                assert!(ui.scroll([8., 8.], [0., 1600.125]).unwrap());
                painter.prepare(&ui, &target.render_format(), 1.).unwrap();
            }
            let before = painter.painter().text().stats().draw_calls;
            let mut frame = target.begin_frame().unwrap();
            {
                let mut pass = frame
                    .render_pass()
                    .clear_color(wgpu::Color::BLACK)
                    .begin()
                    .unwrap();
                pass.set_scissor_rect(0, 0, 60, 64).unwrap();
                painter.paint(&ui, &mut pass, 1.).unwrap();
                assert_eq!(pass.scissor_rect(), [0, 0, 60, 64]);
            }
            let buffer = read_pixel(&graphics, &mut frame, &texture);
            let bytes = pixels(&graphics, &buffer, frame.finish().unwrap());
            let stats = painter.painter().text().stats();
            assert!(
                (6..=8).contains(&(stats.draw_calls - before)),
                "only visible text should draw"
            );
            assert_eq!(stats.geometry_bytes, text_stats.geometry_bytes);
            assert_eq!(stats.uploaded_bytes, text_stats.uploaded_bytes);
            assert_eq!(ui.stats(), ui_stats);
            let has_ink = |left: usize, right: usize| {
                (8..28).any(|y| (left..right).any(|x| bytes[y * 256 + x * 4] > 0))
            };
            assert!(has_ink(36, 46), "visible child of an offscreen parent");
            assert!(
                has_ink(48, 59),
                "glyph overflow beyond a zero-height layout box"
            );
            assert_eq!(
                &bytes[44 * 256 + 44 * 4..44 * 256 + 44 * 4 + 4],
                &[0, 255, 0, 255]
            );
            assert_eq!(
                &bytes[16 * 256 + 62 * 4..16 * 256 + 62 * 4 + 4],
                &[0, 0, 0, 255]
            );
        }
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn real_font_tab_panels_handle_intrinsic_grid_probes_and_constrain_entity_scrolling() {
    struct Document;
    impl View for Document {
        fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            column().fill_width().fill_height().min_height(0.).padding(16.).gap(12.)
                .child(label("Document").font_size(24.))
                .child(text_input("Document name").fill_width())
                .child(text_input("Edit this note; each window keeps its own caret.").fill_width())
                .child(button("Cycle focus within this document"))
                .child(label("Switch tabs to retain selection and scrolling. Arrow keys select headers; Delete proposes closing a tab.").color(ThemeColor::TextMuted))
                .child(scroll_area(column().children((0..100).map(|i|label(format!("Document — row {i:03}"))))))
        }
    }
    struct TabsPage {
        docs: Vec<Entity<Document>>,
        selected: Key,
    }
    impl View for TabsPage {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            tabs()
                .size(640., 400.)
                .selected(self.selected.clone())
                .tabs(
                    self.docs
                        .iter()
                        .enumerate()
                        .map(|(i, doc)| tab(i, format!("Document {i}"), doc.clone())),
                )
                .on_select(cx.listener(|s, e: &TabSelectEvent, _| s.selected = e.key.clone()))
        }
    }
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let errors = graphics
            .device()
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let mut target = graphics
            .create_framebuffer(FramebufferOptions::new(640, 400))
            .unwrap();
        let mut r = Runtime::new();
        let root = r.update(|cx| {
            let docs = (0..2).map(|_| cx.new(|_| Document)).collect();
            cx.new(|_| TabsPage {
                docs,
                selected: 0.into(),
            })
        });
        let mut ui = Ui::new(&mut r, root.clone()).unwrap();
        let mut painter = UiPainter::new(&graphics);
        painter
            .fonts_mut()
            .load_font_shared(Arc::<[u8]>::from(
                include_bytes!("../tests/fonts/SourceSans3-Regular.otf").as_slice(),
            ))
            .unwrap();
        for selected in [0, 1, 0] {
            r.update(|cx| root.update(cx, |s, _| s.selected = selected.into()));
            ui.prepare(&mut r, [640., 400.], &mut painter).unwrap();
            let viewport = ui.elements().find(|e| e.scroll_range[1] > 0.).unwrap();
            assert!(viewport.bounds.height > 0. && viewport.bounds.height < 400.);
            let point = [viewport.bounds.x + 2., viewport.bounds.y + 2.];
            assert!(ui.scroll(point, [0., 20.]).unwrap());
            painter.prepare(&ui, &target.render_format(), 1.).unwrap();
            let mut frame = target.begin_frame().unwrap();
            painter
                .compose(&ui, &mut frame, 1., |frame, composed| {
                    let mut pass = frame
                        .render_pass()
                        .clear_color(wgpu::Color::BLACK)
                        .begin()?;
                    composed.paint(&mut pass)
                })
                .unwrap();
            frame.finish().unwrap();
        }
        assert!(errors.pop().await.is_none());
    });
}
