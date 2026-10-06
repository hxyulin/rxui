use super::*;
use crate::*;
use astrelis::{FramebufferOptions, wgpu};

pub(super) struct Page(pub(super) Element);
impl View for Page {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        self.0.clone()
    }
}
pub(super) fn prepare(
    runtime: &mut Runtime,
    ui: &mut Ui<Page>,
    painter: &mut UiPainter,
    format: &RenderFormat,
) {
    ui.prepare(runtime, [64., 64.], painter).unwrap();
    painter.prepare(ui, format, 1.).unwrap();
}
pub(super) fn read_pixel(
    graphics: &GraphicsContext,
    frame: &mut astrelis::Frame<'_, 'static>,
    texture: &wgpu::Texture,
) -> wgpu::Buffer {
    let buffer = graphics.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("RXUI image readback"),
        size: 64 * 256,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    frame.encoder().copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: Default::default(),
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(64),
            },
        },
        texture.size(),
    );
    buffer
}
pub(super) fn pixels(
    graphics: &GraphicsContext,
    buffer: &wgpu::Buffer,
    submission: wgpu::SubmissionIndex,
) -> Vec<u8> {
    let (tx, rx) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    graphics
        .device()
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(std::time::Duration::from_secs(10)),
        })
        .unwrap();
    rx.recv_timeout(std::time::Duration::from_secs(10))
        .unwrap()
        .unwrap();
    let bytes = buffer.slice(..).get_mapped_range().unwrap().to_vec();
    buffer.unmap();
    bytes
}
#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn shared_images_upload_once_alpha_clips_and_cache_ownership_match_pixels() {
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
        let asset = Image::from_rgba8(1, 1, vec![255, 0, 0, 128]).unwrap();
        let description = || {
            row()
                .child(image(asset.clone()).width(32.).height(32.))
                .child(
                    image(asset.clone())
                        .width(32.)
                        .height(32.)
                        .filter(ImageFilter::Nearest),
                )
        };
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| cx.new(|_| Page(description())));
        let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
        let other =
            runtime.update(|cx| cx.new(|_| Page(image(asset.clone()).width(10.).height(10.))));
        let mut other_ui = Ui::new(&mut runtime, other).unwrap();
        let mut painter = UiPainter::new(&graphics);
        let format = target.render_format();
        prepare(&mut runtime, &mut ui, &mut painter, &format);
        prepare(&mut runtime, &mut other_ui, &mut painter, &format);
        assert_eq!(
            painter.image_stats(),
            ImageStats {
                uploads: 1,
                uploaded_bytes: 4,
                bindings: 2
            }
        );
        let stats = painter.image_stats();
        for _ in 0..3 {
            prepare(&mut runtime, &mut ui, &mut painter, &format);
        }
        runtime
            .update(|cx| root.update(cx, |s, _| s.0 = description().background([0., 0., 1., 1.])));
        prepare(&mut runtime, &mut ui, &mut painter, &format);
        assert_eq!(painter.image_stats(), stats);
        let texture = target.color_texture().unwrap().clone();
        let mut frame = target.begin_frame().unwrap();
        {
            let mut pass = frame
                .render_pass()
                .clear_color(wgpu::Color::BLACK)
                .begin()
                .unwrap();
            pass.set_scissor_rect(4, 4, 56, 56).unwrap();
            painter.paint(&ui, &mut pass, 1.).unwrap();
            assert_eq!(pass.scissor_rect(), [4, 4, 56, 56]);
        }
        let buffer = read_pixel(&graphics, &mut frame, &texture);
        let bytes = pixels(&graphics, &buffer, frame.finish().unwrap());
        let at = |x: usize, y: usize| &bytes[y * 256 + x * 4..y * 256 + x * 4 + 4];
        for x in [10, 50] {
            assert!(at(x, 10)[0].abs_diff(128) <= 1);
            assert!(at(x, 10)[2].abs_diff(127) <= 1);
            assert_eq!(at(x, 10)[3], 255);
        }
        assert_eq!(at(2, 10), [0, 0, 0, 255]);
        assert_eq!(at(10, 40), [0, 0, 0, 255]);
        painter.forget(&ui);
        assert_eq!(painter.images.len(), 1);
        assert_eq!(painter.image_bindings.len(), 1);
        painter.forget(&other_ui);
        assert!(painter.images.is_empty());
        assert!(painter.image_bindings.is_empty());
        assert!(errors.pop().await.is_none());
    });
}
#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn framebuffer_images_follow_resize_msaa_suspension_and_render_in_the_same_submission() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let errors = graphics
            .device()
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let mut source = graphics
            .create_framebuffer(FramebufferOptions::new(8, 8))
            .unwrap();
        let mut target = graphics
            .create_framebuffer(
                FramebufferOptions::new(64, 64)
                    .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC),
            )
            .unwrap();
        let asset = Image::from_framebuffer(&source);
        let identity = asset.id();
        let mut runtime = Runtime::new();
        let root =
            runtime.update(|cx| cx.new(|_| Page(image(asset.clone()).width(32.).height(32.))));
        let mut ui = Ui::new(&mut runtime, root).unwrap();
        let mut painter = UiPainter::new(&graphics);
        prepare(&mut runtime, &mut ui, &mut painter, &target.render_format());
        let layouts = ui.stats().layout_passes;
        for size in [[8, 8], [16, 32], [0, 0], [32, 16]] {
            source.resize(size[0], size[1]).unwrap();
            if size == [32, 16] && source.supported_sample_counts().contains(&4) {
                source.set_sample_count(4).unwrap();
            }
            prepare(&mut runtime, &mut ui, &mut painter, &target.render_format());
            assert_eq!(asset.id(), identity);
            assert_eq!(ui.stats().layout_passes, layouts);
            let texture = target.color_texture().unwrap().clone();
            let mut frame = target.begin_frame().unwrap();
            if size != [0, 0] {
                // Premultiplied half-alpha green over blue; no separate submit/readback.
                let _pass = frame
                    .render_to(&mut source)
                    .clear_color(wgpu::Color {
                        r: 0.,
                        g: 0.5,
                        b: 0.,
                        a: 0.5,
                    })
                    .begin()
                    .unwrap();
            }
            {
                let mut pass = frame
                    .render_pass()
                    .clear_color(wgpu::Color::BLUE)
                    .begin()
                    .unwrap();
                painter.paint(&ui, &mut pass, 1.).unwrap();
            }
            let buffer = read_pixel(&graphics, &mut frame, &texture);
            let bytes = pixels(&graphics, &buffer, frame.finish().unwrap());
            let pixel = &bytes[16 * 256 + 16 * 4..16 * 256 + 16 * 4 + 4];
            if size == [0, 0] {
                assert_eq!(pixel, [0, 0, 255, 255]);
            } else {
                assert!(pixel[1].abs_diff(128) <= 1);
                assert!(pixel[2].abs_diff(127) <= 1);
                assert_eq!(pixel[3], 255);
            }
        }
        assert_eq!(painter.image_stats().uploads, 0);
        // Feedback is rejected by the managed image binding before its draw.
        let mut frame = source.begin_frame().unwrap();
        {
            let mut pass = frame
                .render_pass()
                .clear_color(wgpu::Color::BLACK)
                .begin()
                .unwrap();
            assert!(matches!(
                painter.paint(&ui, &mut pass, 1.),
                Err(UiError::Graphics(astrelis::Error::TextureFeedback))
            ));
        }
        drop(frame);
        let foreign = GraphicsContext::headless().await.unwrap();
        let foreign_target = foreign
            .create_framebuffer(FramebufferOptions::new(8, 8))
            .unwrap();
        let foreign_root =
            runtime.update(|cx| cx.new(|_| Page(image(Image::from_framebuffer(&foreign_target)))));
        let mut foreign_ui = Ui::new(&mut runtime, foreign_root).unwrap();
        foreign_ui
            .prepare(&mut runtime, [64., 64.], &mut painter)
            .unwrap();
        assert!(matches!(
            painter.prepare(&foreign_ui, &target.render_format(), 1.),
            Err(UiError::Graphics(astrelis::Error::DeviceMismatch))
        ));
        assert!(errors.pop().await.is_none());
    });
}
