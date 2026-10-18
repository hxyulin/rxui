use super::GraphicsContext;
use crate::*;
use astrelis::{FramebufferOptions, wgpu};

const SIZE: [u32; 2] = [640, 360];
const LABELS: [&str; 4] = [
    "ROBOT",
    "Robot data unavailable",
    "MINIMAP",
    "Position unavailable",
];

struct Hud(u32);
impl View for Hud {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let panel = || {
            column()
                .absolute()
                .padding(16.)
                .gap(12.)
                .background(rgb8(15, 19, 23))
                .border(1., rgb8(65, 75, 85))
        };
        let mut statuses = column().absolute().left(400.).top(24.).z_index(10);
        if self.0 == 0 {
            statuses = statuses.child(label("NO LINK").key("link-status"));
        }
        statuses = statuses.child(
            label("NO VIDEO")
                .key("video-status")
                .padding(4.)
                .background(rgb8(36, 29, 20))
                .border(1., rgb8(126, 98, 53)),
        );
        stack()
            .size(SIZE[0] as f32, SIZE[1] as f32)
            .color([1.; 4])
            .font_size(20.)
            .child(
                panel()
                    .key("top-band")
                    .left(16.)
                    .top(16.)
                    .size(608., 90.)
                    .child(label(format!("MATCH {:03}", self.0)).font_size(24.)),
            )
            .child(
                panel()
                    .key("robot-panel")
                    .left(16.)
                    .bottom(16.)
                    .size(300., 140.)
                    .child(label(LABELS[0]).font_size(24.))
                    .child(label(LABELS[1])),
            )
            .child(
                panel()
                    .key("minimap-panel")
                    .right(16.)
                    .bottom(16.)
                    .size(300., 140.)
                    .child(label(LABELS[2]).font_size(24.))
                    .child(label(LABELS[3])),
            )
            .child(statuses)
    }
}

#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn retained_hud_glyphs_survive_changing_text_in_mixed_passes() {
    // Model the consumer's telemetry-only path: no image or opacity layers, static
    // labels over filled/bordered panels, a changing match label, and a status
    // transition. Exercise both changed and unchanged whole-placement text passes.
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let errors = graphics
            .device()
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let mut target = graphics
            .create_framebuffer(
                FramebufferOptions::new(SIZE[0], SIZE[1])
                    .format(wgpu::TextureFormat::Rgba8UnormSrgb)
                    .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC),
            )
            .unwrap();
        let texture = target.color_texture().unwrap().clone();
        let row_bytes = SIZE[0] * 4;
        assert_eq!(row_bytes % wgpu::COPY_BYTES_PER_ROW_ALIGNMENT, 0);
        let readback = graphics.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("RXUI retained HUD glyph readback"),
            size: u64::from(row_bytes) * u64::from(SIZE[1]),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut test = TestUi::new(&graphics, SIZE, Hud(0)).unwrap();
        test.painter_mut()
            .register_font(&include_bytes!("../tests/fonts/SourceSans3-Regular.otf")[..])
            .unwrap();
        test.painter_mut()
            .map_family(FontFamily::SansSerif, "Source Sans 3");
        let mut reference = Vec::new();
        let mut layouts = Vec::new();
        for index in 0..512 {
            if index % 8 == 0 {
                let root = test.entity().clone();
                test.runtime()
                    .update(|cx| root.update(cx, |hud, _| hud.0 = index / 8));
            }
            test.prepare().unwrap();
            let (_, ui, painter) = test.parts();
            painter.prepare(ui, &target.render_format(), 1.).unwrap();
            assert!(!painter.text_atlas_full(ui));
            assert_eq!(painter.layer_stats().live_layers, 0);
            let mut frame = target.begin_frame().unwrap();
            painter
                .compose(ui, &mut frame, 1., |frame, composed| {
                    let mut pass = frame
                        .render_pass()
                        .clear_color(wgpu::Color::BLACK)
                        .begin()?;
                    composed.paint(&mut pass)
                })
                .unwrap();
            frame.encoder().copy_texture_to_buffer(
                texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row_bytes),
                        rows_per_image: Some(SIZE[1]),
                    },
                },
                texture.size(),
            );
            let bytes =
                super::image_gpu_tests::pixels(&graphics, &readback, frame.finish().unwrap());
            for (label_index, text) in LABELS.iter().enumerate() {
                let element = ui.elements().find(|e| e.text == Some(*text)).unwrap();
                let resource = &painter.texts[&element.id];
                let layout = resource.prepared_layout.as_ref().unwrap();
                let prepared = resource.prepared.as_ref().unwrap();
                assert!(prepared.glyph_count() > 0);
                assert!(!prepared.atlas_full());
                if index == 0 {
                    layouts.push(layout.clone());
                } else {
                    assert!(std::sync::Arc::ptr_eq(layout, &layouts[label_index]));
                }
                let bounds = element.bounds;
                let mut ink = Vec::new();
                for y in bounds.y.ceil() as usize..(bounds.y + bounds.height).floor() as usize {
                    for x in bounds.x.ceil() as usize..(bounds.x + bounds.width).floor() as usize {
                        let offset = (y * SIZE[0] as usize + x) * 4;
                        ink.push(bytes[offset..offset + 3].iter().all(|&v| v > 180));
                    }
                }
                assert!(
                    ink.iter().filter(|&&p| p).count() > 50,
                    "frame {index}, {text}: missing glyphs"
                );
                if index == 0 {
                    reference.push(ink);
                } else {
                    assert_eq!(
                        ink, reference[label_index],
                        "frame {index}, {text}: static glyph pixels changed"
                    );
                }
            }
        }
        assert!(errors.pop().await.is_none());
    });
}
