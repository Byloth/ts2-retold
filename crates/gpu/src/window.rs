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
    smooth: wgpu::Sampler,
    /// A black pixel, drawn large behind the game's picture.
    black: wgpu::BindGroup,
    surround: Option<Surround>,
    /// The same pipeline, laying a picture over what is there by its
    /// transparency; and the picture laid over the game's, if any.
    pipeline_over: wgpu::RenderPipeline,
    overlay: Option<Laid>,
}

/// A picture of the window's own laid over the game's picture, in its
/// top right corner: rows of red, green, blue, alpha bytes, shown pixel
/// for pixel, `margin` pixels from the corner.
pub struct Overlay<'a> {
    pub rgba: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub margin: u32,
}

struct Laid {
    picture: wgpu::BindGroup,
    width: u32,
    height: u32,
    margin: u32,
}

/// What the window shows around the game's picture when it draws its own
/// frame: a bar across the top and a border along the other sides.
pub struct Bar<'a> {
    /// The bar's picture: rows of red, green, blue, alpha bytes.
    pub rgba: &'a [u8],
    pub width: u32,
    pub height: u32,
    /// The thickness of the border in pixels, and its colour.
    pub border: u32,
    pub border_color: [u8; 3],
}

struct Surround {
    bar: wgpu::BindGroup,
    height: u32,
    border: u32,
    border_color: wgpu::Color,
}

impl Gpu {
    /// Opens the graphics adapter that can draw to a window, and the
    /// window's picture, `width` by `height` pixels. `display` is the
    /// window system the window belongs to.
    pub fn for_window(
        window: impl Into<wgpu::SurfaceTarget<'static>>,
        display: Box<dyn wgpu::wgt::WgpuHasDisplayHandle>,
        width: u32,
        height: u32,
    ) -> Result<(Gpu, Presenter), Error> {
        pollster::block_on(async {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(display));
            let surface =
                instance.create_surface(window).map_err(|error| Error::NoAdapter(error.to_string()))?;
            let adapter = crate::choose_adapter(&instance, Some(&surface)).await?;
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor { label: Some("andy"), ..Default::default() })
                .await
                .map_err(|error| Error::NoDevice(error.to_string()))?;
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
            let gpu = Gpu::with_device(device, queue, &adapter);
            let presenter = Presenter::new(&gpu, surface, configuration);
            Ok((gpu, presenter))
        })
    }
}

fn group(
    gpu: &Gpu,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("shown picture"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    })
}

/// A picture of the window's own (not a surface of the game), ready to be
/// drawn.
fn picture(
    gpu: &Gpu,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> wgpu::BindGroup {
    let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("window picture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        texture.as_image_copy(),
        rgba,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * width), rows_per_image: None },
        size,
    );
    group(gpu, layout, sampler, &texture.create_view(&wgpu::TextureViewDescriptor::default()))
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
        let pipeline_with = |blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipeline = pipeline_with(None);
        let pipeline_over = pipeline_with(Some(wgpu::BlendState::ALPHA_BLENDING));
        // The picture is enlarged without smoothing: its pixels stay pixels.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("show"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        // A picture drawn at a finer grain than its pixels is shown
        // smoothed: its texels are not the game's pixels.
        let smooth = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("show smoothed"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let black = picture(gpu, &layout, &sampler, 1, 1, &[0, 0, 0, 0xFF]);
        Presenter {
            smooth,
            surface,
            configuration,
            pipeline,
            layout,
            sampler,
            black,
            surround: None,
            pipeline_over,
            overlay: None,
        }
    }

    /// Sets what is shown around the game's picture from now on.
    pub fn set_bar(&mut self, gpu: &Gpu, bar: Bar) {
        let [red, green, blue] = bar.border_color.map(|channel| channel as f64 / 255.0);
        self.surround = Some(Surround {
            bar: picture(gpu, &self.layout, &self.sampler, bar.width, bar.height, bar.rgba),
            height: bar.height,
            border: bar.border,
            border_color: wgpu::Color { r: red, g: green, b: blue, a: 1.0 },
        });
    }

    /// Sets the picture laid over the game's from now on, or takes it away.
    pub fn set_overlay(&mut self, gpu: &Gpu, overlay: Option<Overlay>) {
        self.overlay = overlay.map(|overlay| Laid {
            picture: picture(gpu, &self.layout, &self.sampler, overlay.width, overlay.height, overlay.rgba),
            width: overlay.width,
            height: overlay.height,
            margin: overlay.margin,
        });
    }

    /// The window has a new size.
    pub fn resize(&mut self, gpu: &Gpu, width: u32, height: u32) {
        (self.configuration.width, self.configuration.height) = (width.max(1), height.max(1));
        self.surface.configure(&gpu.device, &self.configuration);
    }

    /// Shows the window's picture: the bar and the border, if it has
    /// them, and inside them a surface of the game as large as fits with
    /// its shape kept, black around it. Without a surface the inside is
    /// black: a window is given a picture as soon as it exists, not when
    /// the game first has one (which is seconds later, after it has
    /// loaded). Returns false if the window had no picture to draw on this
    /// time.
    pub fn show(&mut self, gpu: &mut Gpu, surface: Option<SurfaceId>) -> bool {
        gpu.flush();
        gpu.submit();
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                frame
            }
            _ => {
                self.surface.configure(&gpu.device, &self.configuration);
                return false;
            }
        };
        let (window_width, window_height) = (self.configuration.width, self.configuration.height);
        // The room inside the bar and the border.
        let (top, border, around) = match &self.surround {
            Some(surround) => (surround.height.min(window_height), surround.border, surround.border_color),
            None => (0, 0, wgpu::Color::BLACK),
        };
        let left = border.min(window_width) as f32;
        let room_width = window_width.saturating_sub(2 * border) as f32;
        let room_height = window_height.saturating_sub(top + border) as f32;
        let shown = surface.map(|surface| {
            let shown = gpu.surface(surface);
            let sampler = if shown.scale > 1 { &self.smooth } else { &self.sampler };
            (group(gpu, &self.layout, sampler, &shown.view), shown.width as f32, shown.height as f32)
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
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(around), store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            if let (Some(surround), true) = (&self.surround, top > 0) {
                // While the window changes size the bar may be a picture
                // for the old one for a moment; it is stretched to fit.
                pass.set_bind_group(0, &surround.bar, &[]);
                pass.set_viewport(0.0, 0.0, window_width as f32, top as f32, 0.0, 1.0);
                pass.draw(0..3, 0..1);
            }
            if room_width > 0.0 && room_height > 0.0 {
                if self.surround.is_some() {
                    pass.set_bind_group(0, &self.black, &[]);
                    pass.set_viewport(left, top as f32, room_width, room_height, 0.0, 1.0);
                    pass.draw(0..3, 0..1);
                }
                if let Some((group, shown_width, shown_height)) = &shown {
                    let scale = (room_width / shown_width).min(room_height / shown_height);
                    let (width, height) = (shown_width * scale, shown_height * scale);
                    pass.set_bind_group(0, group, &[]);
                    pass.set_viewport(
                        left + (room_width - width) / 2.0,
                        top as f32 + (room_height - height) / 2.0,
                        width,
                        height,
                        0.0,
                        1.0,
                    );
                    pass.draw(0..3, 0..1);
                }
                // The overlay, in the room's top right corner, if it fits.
                if let Some(laid) = &self.overlay {
                    let (width, height, margin) = (laid.width as f32, laid.height as f32, laid.margin as f32);
                    if width + margin <= room_width && height + margin <= room_height {
                        pass.set_pipeline(&self.pipeline_over);
                        pass.set_bind_group(0, &laid.picture, &[]);
                        let x = left + room_width - margin - width;
                        pass.set_viewport(x, top as f32 + margin, width, height, 0.0, 1.0);
                        pass.draw(0..3, 0..1);
                    }
                }
            }
        }
        gpu.queue.submit([encoder.finish()]);
        gpu.queue.present(frame);
        true
    }
}
