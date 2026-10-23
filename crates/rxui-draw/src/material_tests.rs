use std::num::NonZeroU64;

use wgpu::util::DeviceExt;

use super::{
    tests::{pass, pixels, quad},
    *,
};

const SHADER: &str = r#"
struct Parameters { tint: vec4<f32>, offset: vec4<f32> }
@group(0) @binding(0) var<uniform> parameters: Parameters;
struct Output {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}
@vertex
fn vertex_custom(@location(0) position: vec3<f32>, @location(1) color: vec4<f32>) -> Output {
    var output: Output;
    output.position = vec4(position.x * 0.45 + parameters.offset.x, position.yz, 1.0);
    output.color = color * parameters.tint;
    return output;
}
@fragment
fn fragment_custom(input: Output) -> @location(0) vec4<f32> {
    return vec4(input.color.rgb * input.color.a, input.color.a);
}
"#;

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn materials_bind_dynamic_uniforms_and_reuse_prepared_pipelines() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let device = graphics.device();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Custom material test shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: NonZeroU64::new(32),
                },
                count: None,
            }],
        });
        let material = graphics.create_material(
            MaterialOptions::new(&shader)
                .entry_points("vertex_custom", "fragment_custom")
                .bind_group_layouts(&[Some(&layout)]),
        );
        assert_eq!(material.shader(), &shader);
        let stride = (device.limits().min_uniform_buffer_offset_alignment as usize).max(32);
        let mut data = vec![0; stride * 2];
        data[..32].copy_from_slice(bytemuck::cast_slice(&[
            1.0_f32, 0.0, 0.0, 0.5, -0.45, 0.0, 0.0, 0.0,
        ]));
        data[stride..stride + 32].copy_from_slice(bytemuck::cast_slice(&[
            0.0_f32, 0.0, 1.0, 1.0, 0.45, 0.0, 0.0, 0.0,
        ]));
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &data,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: NonZeroU64::new(32),
                }),
            }],
        });
        let mesh = quad(&graphics, [1.0; 4]);
        let green = quad(&graphics, [0.0, 1.0, 0.0, 1.0]);
        let mut renderer = MeshRenderer::new(&graphics);
        let format = wgpu::TextureFormat::Rgba8Unorm;
        renderer
            .prepare(&crate::RenderFormat::color(format, 1))
            .unwrap();
        renderer
            .prepare_material(&material, &crate::RenderFormat::color(format, 1))
            .unwrap();
        let cached = renderer.pipelines.clone();
        renderer
            .prepare_material(&material.clone(), &crate::RenderFormat::color(format, 1))
            .unwrap();
        renderer
            .prepare(&crate::RenderFormat::color(format, 1))
            .unwrap();
        assert_eq!(
            renderer.pipelines, cached,
            "preparation and clones reuse pipelines"
        );
        let result = pixels(&graphics, |encoder, view| {
            let mut pass = pass(
                &graphics,
                encoder,
                view,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            )
            .unwrap();
            pass.as_wgpu().set_bind_group(0, &group, &[0]);
            renderer
                .draw_with_material(&mut pass, &mesh, &material)
                .unwrap();
            pass.set_scissor_rect(30, 0, 4, 64).unwrap();
            renderer.draw(&mut pass, &green).unwrap();
            pass.set_scissor_rect(0, 0, 64, 64).unwrap();
            pass.as_wgpu().set_bind_group(0, &group, &[stride as u32]);
            renderer
                .draw_with_material(&mut pass, &mesh, &material)
                .unwrap();
        });
        let color_at =
            |image: &[u8], x: usize| image[32 * 256 + x * 4..32 * 256 + x * 4 + 4].to_vec();
        assert_eq!(color_at(&result, 16), [128, 0, 0, 255]);
        assert_eq!(color_at(&result, 32), [0, 255, 0, 255]);
        assert_eq!(color_at(&result, 48), [0, 0, 255, 255]);
        assert_eq!(
            renderer.pipelines, cached,
            "prepared draws create no pipelines"
        );

        // Updating uniform contents changes shading without creating a material or pipeline.
        graphics.queue().write_buffer(
            &buffer,
            stride as u64,
            bytemuck::cast_slice(&[1.0_f32, 1.0, 0.0, 1.0]),
        );
        let updated = pixels(&graphics, |encoder, view| {
            let mut pass = pass(
                &graphics,
                encoder,
                view,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            )
            .unwrap();
            pass.as_wgpu().set_bind_group(0, &group, &[stride as u32]);
            renderer
                .draw_with_material(&mut pass, &mesh, &material)
                .unwrap();
        });
        assert_eq!(color_at(&updated, 48), [255, 255, 0, 255]);
        assert_eq!(renderer.pipelines, cached);

        // The same material works with a separately prepared multisampled pipeline.
        renderer
            .prepare_material(&material, &crate::RenderFormat::color(format, 4))
            .unwrap();
        let prepared_msaa = renderer.pipelines.clone();
        let msaa_view =
            crate::target::create_multisample_view(&graphics, format, format, [64, 64], 4).unwrap();
        let multisampled = pixels(&graphics, |encoder, view| {
            let mut pass = RenderPass::new(
                encoder,
                &graphics,
                crate::pass::ManagedColorAttachment {
                    view: &msaa_view,
                    resolve_target: Some(view),
                    format,
                    size: [64, 64],
                    sample_count: 4,
                    resolved_state: None,
                },
                crate::pass::PassOptions::default(),
            )
            .unwrap();
            pass.as_wgpu().set_bind_group(0, &group, &[stride as u32]);
            renderer
                .draw_with_material(&mut pass, &mesh, &material)
                .unwrap();
        });
        assert_eq!(color_at(&multisampled, 48), [255, 255, 0, 255]);
        assert_eq!(renderer.pipelines, prepared_msaa);
        let mut independent = MeshRenderer::new(&graphics);
        let shared = pixels(&graphics, |encoder, view| {
            let mut pass = pass(
                &graphics,
                encoder,
                view,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            )
            .unwrap();
            pass.as_wgpu().set_bind_group(0, &group, &[stride as u32]);
            independent
                .draw_with_material(&mut pass, &mesh, &material)
                .unwrap();
        });
        assert_eq!(
            shared, updated,
            "materials are independent of a particular renderer"
        );
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn material_state_validation_and_device_checks_leave_recording_usable() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let shader = graphics
            .device()
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(include_str!("mesh.wgsl").into()),
            });
        let masked = graphics.create_material(
            MaterialOptions::new(&shader)
                .blend(None)
                .write_mask(wgpu::ColorWrites::RED | wgpu::ColorWrites::ALPHA),
        );
        let culled = graphics
            .create_material(MaterialOptions::new(&shader).cull_mode(Some(wgpu::Face::Front)));
        let reverse = graphics.create_material(
            MaterialOptions::new(&shader)
                .front_face(wgpu::FrontFace::Cw)
                .cull_mode(Some(wgpu::Face::Front)),
        );
        let mesh = quad(&graphics, [1.0, 0.0, 0.0, 0.5]);
        let mut renderer = MeshRenderer::new(&graphics);
        let result = pixels(&graphics, |encoder, view| {
            let mut pass = pass(
                &graphics,
                encoder,
                view,
                wgpu::LoadOp::Clear(wgpu::Color {
                    r: 0.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                }),
            )
            .unwrap();
            renderer
                .draw_with_material(&mut pass, &mesh, &masked)
                .unwrap();
        });
        let center = 32 * 256 + 32 * 4;
        assert_eq!(&result[center..center + 4], &[128, 255, 255, 128]);
        let mut culling = |material: &Material| {
            pixels(&graphics, |encoder, view| {
                let mut pass = pass(
                    &graphics,
                    encoder,
                    view,
                    wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                )
                .unwrap();
                renderer
                    .draw_with_material(&mut pass, &mesh, material)
                    .unwrap();
            })
        };
        let first = culling(&culled);
        let second = culling(&reverse);
        assert_ne!(
            &first[center..center + 4],
            &second[center..center + 4],
            "changing front-face winding changes culling"
        );
        assert!(
            first[center..center + 4] == [0, 0, 0, 255]
                || second[center..center + 4] == [0, 0, 0, 255]
        );

        let cached = renderer.pipelines.clone();
        for count in [0, 3, u32::MAX] {
            assert!(matches!(
                renderer.prepare_material(
                    &masked,
                    &crate::RenderFormat::color(wgpu::TextureFormat::Rgba8Unorm, count)
                ),
                Err(Error::UnsupportedSampleCount { .. })
            ));
            assert!(matches!(
                renderer.prepare(&crate::RenderFormat::color(
                    wgpu::TextureFormat::Rgba8Unorm,
                    count
                )),
                Err(Error::UnsupportedSampleCount { .. })
            ));
        }
        assert!(matches!(
            renderer.prepare(&crate::RenderFormat::color(
                wgpu::TextureFormat::Rgba8Uint,
                1
            )),
            Err(Error::UnsupportedMeshFormat { .. })
        ));
        assert!(matches!(
            renderer.prepare_material(
                &culled,
                &crate::RenderFormat::color(wgpu::TextureFormat::Rgba8Uint, 1)
            ),
            Err(Error::UnsupportedMaterialFormat { .. })
        ));
        assert!(matches!(
            renderer.prepare_material(
                &masked,
                &crate::RenderFormat::color(wgpu::TextureFormat::Depth32Float, 1)
            ),
            Err(Error::UnsupportedColorFormat { .. })
        ));
        assert_eq!(
            renderer.pipelines, cached,
            "rejected preparations do not alter the cache"
        );

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
        let foreign_shader = other
            .device()
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(include_str!("mesh.wgsl").into()),
            });
        let foreign = other.create_material(MaterialOptions::new(&foreign_shader));
        assert!(matches!(
            renderer.prepare_material(
                &foreign,
                &crate::RenderFormat::color(wgpu::TextureFormat::Rgba8Unorm, 1)
            ),
            Err(Error::DeviceMismatch)
        ));
        let usable = pixels(&graphics, |encoder, view| {
            let mut pass = pass(
                &graphics,
                encoder,
                view,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            )
            .unwrap();
            assert!(matches!(
                renderer.draw_with_material(&mut pass, &mesh, &foreign),
                Err(Error::DeviceMismatch)
            ));
            renderer
                .draw_with_material(&mut pass, &mesh, &masked)
                .unwrap();
        });
        assert_eq!(&usable[center..center + 4], &[128, 0, 0, 128]);

        // Shader-interface failures remain inspectable using wgpu's validation scopes.
        let invalid = graphics.create_material(
            MaterialOptions::new(&shader).entry_points("missing", "fragment_main"),
        );
        let scope = graphics
            .device()
            .push_error_scope(wgpu::ErrorFilter::Validation);
        renderer
            .prepare_material(
                &invalid,
                &crate::RenderFormat::color(wgpu::TextureFormat::Rgba8Unorm, 1),
            )
            .unwrap();
        let error = scope
            .pop()
            .await
            .expect("wgpu must report the missing entry point");
        assert!(error.to_string().contains("missing"));
    });
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn custom_integer_material_renders_to_integer_framebuffer() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let shader = graphics.device().create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(r#"
                @vertex fn vertex_main(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
                    return vec4(position, 1.0);
                }
                @fragment fn fragment_main() -> @location(0) vec4<u32> {
                    return vec4<u32>(7u, 23u, 255u, 64u);
                }
            "#.into()),
        });
        let material = graphics.create_material(MaterialOptions::new(&shader).blend(None));
        let mut framebuffer = graphics
            .create_framebuffer(
                crate::FramebufferOptions::new(1, 1)
                    .format(wgpu::TextureFormat::Rgba8Uint)
                    .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC),
            )
            .unwrap();
        let texture = framebuffer.color_texture().unwrap().clone();
        let buffer = graphics.device().create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut renderer = MeshRenderer::new(&graphics);
        renderer
            .prepare_material(
                &material,
                &crate::RenderFormat::color(framebuffer.format(), framebuffer.sample_count()),
            )
            .unwrap();
        let mesh = quad(&graphics, [1.0; 4]);
        let mut frame = framebuffer.begin_frame().unwrap();
        {
            let mut pass = frame.render_pass().begin().unwrap();
            renderer
                .draw_with_material(&mut pass, &mesh, &material)
                .unwrap();
        }
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
                    bytes_per_row: Some(256),
                    rows_per_image: Some(1),
                },
            },
            texture.size(),
        );
        let submission = frame.finish().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
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
        {
            let result = buffer.slice(..).get_mapped_range().unwrap();
            assert_eq!(&result[..4], &[7, 23, 255, 64]);
        }
        buffer.unmap();
    });
}
