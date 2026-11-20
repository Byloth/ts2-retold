//! Showing what the game presents in a window.

use andy_runtime::video::SurfaceId;

use crate::{Error, Gpu};

/// The picture of a window: where presented surfaces are shown.
pub struct Presenter {
    surface: wgpu::Surface<'static>,
    configuration: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl Gpu {
    /// Opens the graphics adapter that can draw to a window, and the
    /// window's picture, `width` by `height` pixels.
    pub fn for_window(
        window: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
    ) -> Result<(Gpu, Presenter), Error> {
        pollster::block_on(async {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
            let surface =
                instance.create_surface(window).map_err(|error| Error::NoAdapter(error.to_string()))?;
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: Some(&surface),
                    ..Default::default()
                })
                .await
                .map_err(|error| Error::NoAdapter(error.to_string()))?;
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor { label: Some("andy"), ..Default::default() })
                .await
                .map_err(|error| Error::NoDevice(error.to_string()))?;
            let info = adapter.get_info();
            let capabilities = surface.get_capabilities(&adapter);
            // The surfaces hold colours as the display shows them, so the
            // window's format must not convert them again.
            let format = capabilities
                .formats
                .iter()
                .copied()
                .find(|format| !format.is_srgb())
                .or(capabilities.formats.first().copied())
                .ok_or_else(|| Error::NoAdapter("the window offers no picture format".into()))?;
            let configuration = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                width: width.max(1),
                height: height.max(1),
                present_mode: wgpu::PresentMode::AutoVsync,
                desired_maximum_frame_latency: 2,
                alpha_mode: capabilities.alpha_modes[0],
                view_formats: vec![],
                color_space: wgpu::SurfaceColorSpace::Auto,
            };
            surface.configure(&device, &configuration);
            let gpu = Gpu::with_device(device, queue, format!("{} ({:?})", info.name, info.backend));
            let presenter = Presenter::new(&gpu, surface, configuration);
            Ok((gpu, presenter))
        })
    }
}

impl Presenter {
    fn new(
        gpu: &Gpu,
        surface: wgpu::Surface<'static>,
        configuration: wgpu::SurfaceConfiguration,
    ) -> Presenter {
        let device = &gpu.device;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shown surface"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("show"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("show"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &gpu.shader,
                entry_point: Some("vertex_show"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &gpu.shader,
                entry_point: Some("fragment_show"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: configuration.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        // The picture is enlarged without smoothing: its pixels stay pixels.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("show"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Presenter { surface, configuration, pipeline, layout, sampler }
    }

    /// The window has a new size.
    pub fn resize(&mut self, gpu: &Gpu, width: u32, height: u32) {
        (self.configuration.width, self.configuration.height) = (width.max(1), height.max(1));
        self.surface.configure(&gpu.device, &self.configuration);
    }

    /// Shows a surface in the window, as large as fits with its shape kept,
    /// black around it. Returns false if the window had no picture to draw
    /// on this time.
    pub fn show(&mut self, gpu: &mut Gpu, surface: SurfaceId) -> bool {
        gpu.flush();
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                frame
            }
            _ => {
                self.surface.configure(&gpu.device, &self.configuration);
                return false;
            }
        };
        let shown = gpu.surface(surface);
        let (window_width, window_height) =
            (self.configuration.width as f32, self.configuration.height as f32);
        let scale = (window_width / shown.width as f32).min(window_height / shown.height as f32);
        let (width, height) = (shown.width as f32 * scale, shown.height as f32 * scale);
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shown surface"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&shown.view),
                },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("show"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.set_viewport(
                (window_width - width) / 2.0,
                (window_height - height) / 2.0,
                width,
                height,
                0.0,
                1.0,
            );
            pass.draw(0..3, 0..1);
        }
        gpu.queue.submit([encoder.finish()]);
        gpu.queue.present(frame);
        true
    }
}
