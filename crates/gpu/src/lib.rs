//! The video backend on the GPU (ADR 0012, ADR 0014).
//!
//! Every surface is a texture: colour surfaces hold 8 bits per channel
//! whatever their format, depth surfaces 16 bits, palette-index surfaces one
//! byte. Fills and copies are small render passes. Triangles are collected
//! and drawn together: consecutive batches for the same target go into one
//! render pass, which is ended only when something else needs the surfaces
//! (a fill, a copy, a transfer, a present).
//!
//! Reading a surface back ([`Video::download`]) waits for the GPU. That is
//! possible on native targets only; in a browser it needs a way to suspend
//! (`docs/BACKLOG.md`).

mod window;

use std::collections::HashMap;
use std::num::NonZeroU64;

use andy_runtime::video::{
    Address, Argument, BlendFactor, Combine, Compare, Cull, DepthState, Draw, DrawState, Filter, Operation,
    PixelFormat, Rect, Source, SurfaceId, Vertex, Video,
};

pub use window::{Bar, Overlay, Presenter};

const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const INDEX_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth16Unorm;
/// Rows of a buffer a texture is copied to start at multiples of this.
const ROW_ALIGNMENT: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

/// Why the backend could not start.
#[derive(Debug)]
pub enum Error {
    /// No graphics adapter the backend can use.
    NoAdapter(String),
    NoDevice(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NoAdapter(why) => write!(f, "no usable graphics adapter: {why}"),
            Error::NoDevice(why) => write!(f, "the graphics device could not be opened: {why}"),
        }
    }
}

impl std::error::Error for Error {}

struct Surface {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
    format: PixelFormat,
}

impl Surface {
    fn texture_format(&self) -> wgpu::TextureFormat {
        texture_format(self.format)
    }

    /// Bytes per pixel as the texture holds them.
    fn texel_size(&self) -> u32 {
        match self.texture_format() {
            COLOR_FORMAT => 4,
            DEPTH_FORMAT => 2,
            _ => 1,
        }
    }

    fn aspect(&self) -> wgpu::TextureAspect {
        if self.format == PixelFormat::Depth16 {
            wgpu::TextureAspect::DepthOnly
        } else {
            wgpu::TextureAspect::All
        }
    }

    fn clamp(&self, area: Rect) -> Rect {
        let (x, y) = (area.x.min(self.width), area.y.min(self.height));
        Rect { x, y, width: area.width.min(self.width - x), height: area.height.min(self.height - y) }
    }
}

fn texture_format(format: PixelFormat) -> wgpu::TextureFormat {
    match format {
        PixelFormat::Indexed8 => INDEX_FORMAT,
        PixelFormat::Depth16 => DEPTH_FORMAT,
        _ => COLOR_FORMAT,
    }
}

/// A raw pixel as the four floats a shader sees.
fn shader_color(format: PixelFormat, raw: u32) -> [f32; 4] {
    match format.to_argb(raw) {
        Some(argb) => {
            let [blue, green, red, alpha] = argb.to_le_bytes().map(|channel| channel as f32 / 255.0);
            [red, green, blue, alpha]
        }
        // A palette index is the one channel of its texture.
        None => [(raw & 0xFF) as f32 / 255.0, 0.0, 0.0, 1.0],
    }
}

fn adapter_name(adapter: &wgpu::Adapter) -> String {
    let info = adapter.get_info();
    format!("{} ({:?}, {:?})", info.name, info.backend, info.device_type)
}

/// The graphics adapter to use: one that can draw to the window, if there
/// is a window; a real graphics card before a software renderer; and among
/// equals Vulkan, Direct3D 12, Metal, then OpenGL. `ANDY_BACKEND` (vulkan,
/// dx12, metal or gl) puts one backend first.
///
/// The system's own choice is not taken because it does not look at what
/// kind of device is behind a backend: under WSL it picks Vulkan, which
/// there is a software renderer, while OpenGL reaches the graphics card
/// (with `GALLIUM_DRIVER=d3d12`).
async fn choose_adapter(
    instance: &wgpu::Instance,
    window: Option<&wgpu::Surface<'_>>,
) -> Result<wgpu::Adapter, Error> {
    let wanted = std::env::var("ANDY_BACKEND").unwrap_or_default().to_ascii_lowercase();
    let mut adapters = instance.enumerate_adapters(wgpu::Backends::all()).await;
    if let Some(window) = window {
        adapters.retain(|adapter| adapter.is_surface_supported(window));
    }
    adapters.sort_by_key(|adapter| {
        let info = adapter.get_info();
        let named = !wanted.is_empty() && format!("{:?}", info.backend).to_ascii_lowercase() == wanted;
        let order = match info.backend {
            wgpu::Backend::Vulkan => 0,
            wgpu::Backend::Dx12 => 1,
            wgpu::Backend::Metal => 2,
            _ => 3,
        };
        (!named, info.device_type == wgpu::DeviceType::Cpu, order)
    });
    adapters.into_iter().next().ok_or_else(|| Error::NoAdapter("none was found".into()))
}

/// What a draw pipeline depends on. Everything else is uniforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PipelineKey {
    /// Whether the render pass has a depth surface attached.
    depth_attached: bool,
    depth: Option<DepthState>,
    blend: Option<(BlendFactor, BlendFactor)>,
    cull: Cull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SamplerKey {
    magnify: Filter,
    minify: Filter,
    address_u: Address,
    address_v: Address,
}

/// The pipelines of fills and copies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum RectPipeline {
    Fill(wgpu::TextureFormat),
    FillDepth,
    Copy(wgpu::TextureFormat),
}

/// The uniforms of a batch of triangles; `DrawUniforms` in the shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DrawUniforms {
    color: [u32; 4],
    alpha: [u32; 4],
    test: [u32; 4],
    key: [f32; 4],
    fog: [f32; 4],
    nearest: [u32; 4],
}

/// The uniforms of a fill or copy; `RectUniforms` in the shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct RectUniforms {
    value: [f32; 4],
    source: [u32; 4],
    destination: [u32; 4],
    keyed: [u32; 4],
}

fn combiner(combine: Combine) -> [u32; 4] {
    let operation = match combine.operation {
        Operation::SelectFirst => 0,
        Operation::SelectSecond => 1,
        Operation::Modulate => 2,
        Operation::Modulate2x => 3,
        Operation::Modulate4x => 4,
        Operation::Add => 5,
        Operation::AddSigned => 6,
        Operation::Subtract => 7,
        Operation::BlendDiffuseAlpha => 8,
        Operation::BlendTextureAlpha => 9,
        Operation::BlendCurrentAlpha => 10,
    };
    let argument = |argument: Argument| {
        let source = match argument.source {
            Source::Diffuse => 0,
            Source::Current => 1,
            Source::Texture => 2,
            Source::Specular => 3,
        };
        source | (argument.complement as u32) << 4 | (argument.alpha_replicate as u32) << 5
    };
    [operation, argument(combine.first), argument(combine.second), 0]
}

fn compare(compare: Compare) -> wgpu::CompareFunction {
    match compare {
        Compare::Never => wgpu::CompareFunction::Never,
        Compare::Less => wgpu::CompareFunction::Less,
        Compare::Equal => wgpu::CompareFunction::Equal,
        Compare::LessEqual => wgpu::CompareFunction::LessEqual,
        Compare::Greater => wgpu::CompareFunction::Greater,
        Compare::NotEqual => wgpu::CompareFunction::NotEqual,
        Compare::GreaterEqual => wgpu::CompareFunction::GreaterEqual,
        Compare::Always => wgpu::CompareFunction::Always,
    }
}

/// The alpha test's function as the shader numbers it.
fn alpha_function(compare: Compare) -> u32 {
    match compare {
        Compare::Never => 1,
        Compare::Less => 2,
        Compare::Equal => 3,
        Compare::LessEqual => 4,
        Compare::Greater => 5,
        Compare::NotEqual => 6,
        Compare::GreaterEqual => 7,
        Compare::Always => 8,
    }
}

fn blend_factor(factor: BlendFactor) -> wgpu::BlendFactor {
    match factor {
        BlendFactor::Zero => wgpu::BlendFactor::Zero,
        BlendFactor::One => wgpu::BlendFactor::One,
        BlendFactor::SourceColor => wgpu::BlendFactor::Src,
        BlendFactor::InverseSourceColor => wgpu::BlendFactor::OneMinusSrc,
        BlendFactor::SourceAlpha => wgpu::BlendFactor::SrcAlpha,
        BlendFactor::InverseSourceAlpha => wgpu::BlendFactor::OneMinusSrcAlpha,
        BlendFactor::TargetAlpha => wgpu::BlendFactor::DstAlpha,
        BlendFactor::InverseTargetAlpha => wgpu::BlendFactor::OneMinusDstAlpha,
        BlendFactor::TargetColor => wgpu::BlendFactor::Dst,
        BlendFactor::InverseTargetColor => wgpu::BlendFactor::OneMinusDst,
        BlendFactor::SourceAlphaSaturated => wgpu::BlendFactor::SrcAlphaSaturated,
    }
}

/// One batch of triangles waiting to be drawn.
struct Command {
    pipeline: PipelineKey,
    viewport: Rect,
    texture: Option<(SurfaceId, SamplerKey)>,
    indices: std::ops::Range<u32>,
    base_vertex: i32,
}

/// Triangles collected for one render pass.
#[derive(Default)]
struct Pending {
    target: Option<(SurfaceId, Option<SurfaceId>)>,
    vertices: Vec<Vertex>,
    indices: Vec<u16>,
    uniforms: Vec<DrawUniforms>,
    commands: Vec<Command>,
}

/// How many rectangles (fills, copies) one submission has uniforms for.
const RECT_SLOTS: u64 = 256;

/// How much of each buffer the work of the pending submission uses: bytes
/// of vertices, of indices and of draw uniforms, and rectangles' uniforms.
#[derive(Debug, Default, Clone, Copy)]
struct Used {
    vertices: u64,
    indices: u64,
    uniforms: u64,
    rects: u64,
}

/// A buffer that is replaced by a larger one when it is too small.
struct Growing {
    buffer: wgpu::Buffer,
    usage: wgpu::BufferUsages,
    label: &'static str,
}

impl Growing {
    fn new(device: &wgpu::Device, label: &'static str, usage: wgpu::BufferUsages, size: u64) -> Growing {
        let usage = usage | wgpu::BufferUsages::COPY_DST;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage,
            mapped_at_creation: false,
        });
        Growing { buffer, usage, label }
    }

    /// Makes room for `size` bytes; true if the buffer was replaced.
    fn reserve(&mut self, device: &wgpu::Device, size: u64) -> bool {
        if size <= self.buffer.size() {
            return false;
        }
        *self = Growing::new(device, self.label, self.usage, size.next_power_of_two());
        true
    }
}

/// How much work the backend has done.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Statistics {
    /// Render passes of triangles.
    pub passes: u64,
    pub batches: u64,
    pub triangles: u64,
    pub frames_presented: u64,
}

/// The video backend on the GPU.
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter_name: String,
    /// Whether depth textures can be copied to and from buffers. Where they
    /// cannot (OpenGL), their values travel through a texture of numbers.
    depth_copies: bool,
    /// The pipelines of that detour: to a depth surface, and from one.
    depth_detours: HashMap<bool, (wgpu::RenderPipeline, wgpu::BindGroupLayout)>,
    rect_uniform_layout: wgpu::BindGroupLayout,
    surfaces: Vec<Option<Surface>>,
    shader: wgpu::ShaderModule,
    texture_layout: wgpu::BindGroupLayout,
    source_layout: wgpu::BindGroupLayout,
    draw_layout: wgpu::PipelineLayout,
    rect_layout: wgpu::PipelineLayout,
    draw_uniform_layout: wgpu::BindGroupLayout,
    pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,
    rect_pipelines: HashMap<RectPipeline, wgpu::RenderPipeline>,
    samplers: HashMap<SamplerKey, wgpu::Sampler>,
    /// Bind groups of textures as triangles read them. Without a surface:
    /// the white texture.
    texture_groups: HashMap<Option<(SurfaceId, SamplerKey)>, wgpu::BindGroup>,
    white: wgpu::TextureView,
    pending: Pending,
    vertices: Growing,
    indices: Growing,
    uniforms: Growing,
    uniform_group: wgpu::BindGroup,
    /// Bytes from one batch's uniforms to the next in the uniform buffer.
    uniform_stride: u64,
    rect_uniforms: wgpu::Buffer,
    rect_group: wgpu::BindGroup,
    /// Bytes from one rectangle's uniforms to the next in their buffer.
    rect_stride: u64,
    /// Work recorded and not yet given to the graphics adapter: it is
    /// given all at once ([`Gpu::submit`]), a frame's worth as a rule.
    finished: Vec<wgpu::CommandBuffer>,
    /// Where, in the buffers, the work recorded since the last
    /// submission ends: what is recorded next writes after it, since
    /// everything written to a buffer is there before any of the
    /// submission's work runs.
    used: Used,
    /// The surface last presented.
    pub presented: Option<SurfaceId>,
    pub statistics: Statistics,
}

impl Gpu {
    /// Opens the default graphics adapter.
    pub fn new() -> Result<Gpu, Error> {
        pollster::block_on(Gpu::open())
    }

    async fn open() -> Result<Gpu, Error> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = choose_adapter(&instance, None).await?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor { label: Some("andy"), ..Default::default() })
            .await
            .map_err(|error| Error::NoDevice(error.to_string()))?;
        Ok(Gpu::with_device(device, queue, &adapter))
    }

    /// The backend on a device opened by the caller, who may share it with
    /// a window.
    pub fn with_device(device: wgpu::Device, queue: wgpu::Queue, adapter: &wgpu::Adapter) -> Gpu {
        let adapter_name = adapter_name(adapter);
        let depth_copies = adapter
            .get_downlevel_capabilities()
            .flags
            .contains(wgpu::DownlevelFlags::DEPTH_TEXTURE_AND_BUFFER_COPIES);
        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));
        let uniform_entry = |size: usize, dynamic: bool| wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: dynamic,
                min_binding_size: NonZeroU64::new(size as u64),
            },
            count: None,
        };
        let texture_entry = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler_entry = wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let layout = |label: &str, entries: &[wgpu::BindGroupLayoutEntry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries })
        };
        let draw_uniform_layout =
            layout("draw uniforms", &[uniform_entry(std::mem::size_of::<DrawUniforms>(), true)]);
        let rect_uniform_layout =
            layout("rectangle uniforms", &[uniform_entry(std::mem::size_of::<RectUniforms>(), true)]);
        let texture_layout = layout("texture", &[texture_entry, sampler_entry]);
        let source_layout = layout("copy source", &[texture_entry]);
        let pipeline_layout = |label: &str, groups: &[Option<&wgpu::BindGroupLayout>]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: groups,
                immediate_size: 0,
            })
        };
        let draw_layout = pipeline_layout("draw", &[Some(&draw_uniform_layout), Some(&texture_layout)]);
        let rect_layout = pipeline_layout("rectangle", &[Some(&rect_uniform_layout), Some(&source_layout)]);

        let white = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("white"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            white.as_image_copy(),
            &[0xFF; 4],
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: None },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
        let white = white.create_view(&wgpu::TextureViewDescriptor::default());

        let uniform_stride = (std::mem::size_of::<DrawUniforms>() as u64)
            .next_multiple_of(device.limits().min_uniform_buffer_offset_alignment as u64);
        let vertices = Growing::new(&device, "vertices", wgpu::BufferUsages::VERTEX, 1 << 16);
        let indices = Growing::new(&device, "indices", wgpu::BufferUsages::INDEX, 1 << 14);
        let uniforms =
            Growing::new(&device, "draw uniforms", wgpu::BufferUsages::UNIFORM, 64 * uniform_stride);
        let uniform_group = Gpu::uniform_group(&device, &draw_uniform_layout, &uniforms.buffer);
        let rect_stride = (std::mem::size_of::<RectUniforms>() as u64)
            .next_multiple_of(device.limits().min_uniform_buffer_offset_alignment as u64);
        let rect_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rectangle uniforms"),
            size: RECT_SLOTS * rect_stride,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let rect_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rectangle uniforms"),
            layout: &rect_uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &rect_uniforms,
                    offset: 0,
                    size: NonZeroU64::new(std::mem::size_of::<RectUniforms>() as u64),
                }),
            }],
        });
        Gpu {
            device,
            queue,
            adapter_name,
            depth_copies,
            depth_detours: HashMap::new(),
            rect_uniform_layout,
            surfaces: Vec::new(),
            shader,
            texture_layout,
            source_layout,
            draw_layout,
            rect_layout,
            draw_uniform_layout,
            pipelines: HashMap::new(),
            rect_pipelines: HashMap::new(),
            samplers: HashMap::new(),
            texture_groups: HashMap::new(),
            white,
            pending: Pending::default(),
            vertices,
            indices,
            uniforms,
            uniform_group,
            uniform_stride,
            rect_uniforms,
            rect_group,
            rect_stride,
            finished: Vec::new(),
            used: Used::default(),
            presented: None,
            statistics: Statistics::default(),
        }
    }

    /// The graphics adapter in use and the API it is driven through.
    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    fn uniform_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        buffer: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("draw uniforms"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer,
                    offset: 0,
                    size: NonZeroU64::new(std::mem::size_of::<DrawUniforms>() as u64),
                }),
            }],
        })
    }

    /// Gives the graphics adapter the work recorded so far. Done once a
    /// frame, and before anything that must come after that work: a
    /// surface read back or written from outside, a surface destroyed.
    pub fn submit(&mut self) {
        if !self.finished.is_empty() {
            self.queue.submit(self.finished.drain(..));
        }
        self.used = Used::default();
    }

    /// Writes a rectangle's uniforms where no work pending uses the
    /// buffer, and says where: the offset to bind them at.
    fn rect_uniforms(&mut self, uniforms: &RectUniforms) -> u32 {
        if self.used.rects == RECT_SLOTS {
            self.submit();
        }
        let offset = self.used.rects * self.rect_stride;
        self.queue.write_buffer(&self.rect_uniforms, offset, bytemuck::bytes_of(uniforms));
        self.used.rects += 1;
        offset as u32
    }

    fn surface(&self, id: SurfaceId) -> &Surface {
        self.surfaces[id.0 as usize].as_ref().expect("the surface exists")
    }

    /// The colour surface's pixels as rows of red, green, blue, alpha bytes,
    /// at the precision the GPU holds them.
    pub fn read_rgba(&mut self, id: SurfaceId) -> Vec<u8> {
        self.flush();
        let surface = self.surface(id);
        assert_eq!(surface.texture_format(), COLOR_FORMAT, "a colour surface");
        let area = Rect::whole(surface.width, surface.height);
        self.read_texels(id, area)
    }

    /// Reads a rectangle of a surface's texture: rows of texels without padding.
    fn read_texels(&mut self, id: SurfaceId, area: Rect) -> Vec<u8> {
        let surface = self.surface(id);
        if surface.format == PixelFormat::Depth16 && !self.depth_copies {
            let numbers = self.numbers_from_depth(id, area);
            self.submit();
            return self.read_texture(
                &numbers,
                wgpu::TextureAspect::All,
                2,
                Rect::whole(area.width, area.height),
            );
        }
        let (texture, aspect, size) = (surface.texture.clone(), surface.aspect(), surface.texel_size());
        // The work recorded so far runs before the texture is read.
        self.submit();
        self.read_texture(&texture, aspect, size, area)
    }

    /// Reads a rectangle of a texture: rows of texels without padding.
    /// The work recorded before has been given to the adapter.
    fn read_texture(
        &self,
        texture: &wgpu::Texture,
        aspect: wgpu::TextureAspect,
        texel_size: u32,
        area: Rect,
    ) -> Vec<u8> {
        let row_bytes = area.width * texel_size;
        let padded = row_bytes.next_multiple_of(ROW_ALIGNMENT);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("read back"),
            size: padded as u64 * area.height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: area.x, y: area.y, z: 0 },
                aspect,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d { width: area.width, height: area.height, depth_or_array_layers: 1 },
        );
        self.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |result| result.expect("the read-back buffer maps"));
        self.device
            .poll(wgpu::PollType::Wait { submission_index: None, timeout: None })
            .expect("the device answers");
        let mapped = slice.get_mapped_range().expect("the buffer was just mapped");
        let mut texels = Vec::with_capacity((row_bytes * area.height) as usize);
        for row in mapped.chunks(padded as usize) {
            texels.extend_from_slice(&row[..row_bytes as usize]);
        }
        texels
    }

    /// The pipeline that turns numbers into depth (`to_depth`) or depth
    /// into numbers, and the layout of the texture it reads.
    fn depth_detour(&mut self, to_depth: bool) {
        if self.depth_detours.contains_key(&to_depth) {
            return;
        }
        // Numbers are read as unsigned integers, depth as unfiltered floats.
        let sample_type = if to_depth {
            wgpu::TextureSampleType::Uint
        } else {
            wgpu::TextureSampleType::Float { filterable: false }
        };
        let layout = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("depth detour"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("depth detour"),
            bind_group_layouts: &[Some(&self.rect_uniform_layout), Some(&layout)],
            immediate_size: 0,
        });
        let numbers = [Some(wgpu::ColorTargetState {
            format: wgpu::TextureFormat::R16Uint,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("depth detour"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vertex_cover"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: to_depth.then(|| wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some(if to_depth {
                    "fragment_depth_from_numbers"
                } else {
                    "fragment_numbers_from_depth"
                }),
                compilation_options: Default::default(),
                targets: if to_depth { &[] } else { &numbers },
            }),
            multiview_mask: None,
            cache: None,
        });
        self.depth_detours.insert(to_depth, (pipeline, layout));
    }

    /// Runs one of the depth detours: a triangle over `area` of the target
    /// that reads `source`.
    fn run_depth_detour(
        &mut self,
        to_depth: bool,
        source: &wgpu::TextureView,
        target: &wgpu::TextureView,
        area: Rect,
        uniforms: RectUniforms,
    ) {
        self.depth_detour(to_depth);
        let offset = self.rect_uniforms(&uniforms);
        let (pipeline, layout) = &self.depth_detours[&to_depth];
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("depth detour"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source),
            }],
        });
        let keep = wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store };
        let color = [Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })];
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("depth detour"),
                color_attachments: if to_depth { &[] } else { &color },
                depth_stencil_attachment: to_depth.then_some(wgpu::RenderPassDepthStencilAttachment {
                    view: target,
                    depth_ops: Some(keep),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.rect_group, &[offset]);
            pass.set_bind_group(1, &group, &[]);
            pass.set_viewport(area.x as f32, area.y as f32, area.width as f32, area.height as f32, 0.0, 1.0);
            pass.set_scissor_rect(area.x, area.y, area.width, area.height);
            pass.draw(0..3, 0..1);
        }
        self.finished.push(encoder.finish());
    }

    fn numbers_texture(&self, width: u32, height: u32, usage: wgpu::TextureUsages) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth as numbers"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R16Uint,
            usage,
            view_formats: &[],
        })
    }

    /// A rectangle of a depth surface as a texture of 16-bit numbers.
    fn numbers_from_depth(&mut self, id: SurfaceId, area: Rect) -> wgpu::Texture {
        let numbers = self.numbers_texture(
            area.width,
            area.height,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let target = numbers.create_view(&wgpu::TextureViewDescriptor::default());
        let source = self.surface(id).view.clone();
        let uniforms = RectUniforms {
            value: [0.0; 4],
            source: [area.x, area.y, 0, 0],
            destination: [0; 4],
            keyed: [0; 4],
        };
        self.run_depth_detour(false, &source, &target, Rect::whole(area.width, area.height), uniforms);
        numbers
    }

    /// Writes rows of 16-bit numbers into a rectangle of a depth surface.
    fn depth_from_numbers(&mut self, id: SurfaceId, area: Rect, texels: &[u8]) {
        let numbers = self.numbers_texture(
            area.width,
            area.height,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        );
        self.queue.write_texture(
            numbers.as_image_copy(),
            texels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(2 * area.width),
                rows_per_image: None,
            },
            wgpu::Extent3d { width: area.width, height: area.height, depth_or_array_layers: 1 },
        );
        let source = numbers.create_view(&wgpu::TextureViewDescriptor::default());
        let target = self.surface(id).view.clone();
        let uniforms = RectUniforms {
            value: [0.0; 4],
            source: [0; 4],
            destination: [area.x, area.y, 0, 0],
            keyed: [0; 4],
        };
        self.run_depth_detour(true, &source, &target, area, uniforms);
    }

    fn sampler(&mut self, key: SamplerKey) {
        let filter = |filter| match filter {
            Filter::Nearest => wgpu::FilterMode::Nearest,
            Filter::Linear => wgpu::FilterMode::Linear,
        };
        let address = |address| match address {
            Address::Wrap => wgpu::AddressMode::Repeat,
            Address::Mirror => wgpu::AddressMode::MirrorRepeat,
            Address::Clamp => wgpu::AddressMode::ClampToEdge,
        };
        self.samplers.entry(key).or_insert_with(|| {
            self.device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("texture"),
                address_mode_u: address(key.address_u),
                address_mode_v: address(key.address_v),
                mag_filter: filter(key.magnify),
                min_filter: filter(key.minify),
                ..Default::default()
            })
        });
    }

    fn texture_group(&mut self, texture: Option<(SurfaceId, SamplerKey)>) {
        if self.texture_groups.contains_key(&texture) {
            return;
        }
        // Without a texture the sampler does not matter.
        let nearest = SamplerKey {
            magnify: Filter::Nearest,
            minify: Filter::Nearest,
            address_u: Address::Wrap,
            address_v: Address::Wrap,
        };
        let key = texture.map_or(nearest, |(_, key)| key);
        self.sampler(key);
        let view = match texture {
            Some((surface, _)) => &self.surface(surface).view,
            None => &self.white,
        };
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("texture"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.samplers[&key]),
                },
            ],
        });
        self.texture_groups.insert(texture, group);
    }

    fn pipeline(&mut self, key: PipelineKey) {
        if self.pipelines.contains_key(&key) {
            return;
        }
        let (vertex, fragment) = ("vertex_smooth", "fragment_smooth");
        let blend = key.blend.map(|(source, target)| {
            let component = wgpu::BlendComponent {
                src_factor: blend_factor(source),
                dst_factor: blend_factor(target),
                operation: wgpu::BlendOperation::Add,
            };
            wgpu::BlendState { color: component, alpha: component }
        });
        // The API decides front and back from the winding on the screen,
        // the same thing the state names; counter-clockwise is its "front".
        // (`culling_goes_by_the_winding_on_the_screen` holds this in place.)
        let cull_mode = match key.cull {
            Cull::None => None,
            Cull::Clockwise => Some(wgpu::Face::Back),
            Cull::CounterClockwise => Some(wgpu::Face::Front),
        };
        // With a depth surface attached every pipeline names its format;
        // one that does not use depth always passes and never writes.
        let depth_stencil = key.depth_attached.then(|| {
            let (function, write, bias) =
                key.depth.map_or((wgpu::CompareFunction::Always, false, 0), |depth| {
                    (compare(depth.compare), depth.write, depth.bias)
                });
            wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(write),
                depth_compare: Some(function),
                stencil: wgpu::StencilState::default(),
                // Towards the viewer, in steps of the depth format.
                bias: wgpu::DepthBiasState { constant: -(bias as i32), ..Default::default() },
            }
        });
        let attributes =
            wgpu::vertex_attr_array![0 => Float32x4, 1 => Unorm8x4, 2 => Unorm8x4, 3 => Float32x2];
        let pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("triangles"),
            layout: Some(&self.draw_layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some(vertex),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode,
                ..Default::default()
            },
            depth_stencil,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some(fragment),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR_FORMAT,
                    blend,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        self.pipelines.insert(key, pipeline);
    }

    fn rect_pipeline(&mut self, key: RectPipeline) {
        if self.rect_pipelines.contains_key(&key) {
            return;
        }
        let (fragment, format) = match key {
            RectPipeline::Fill(format) => ("fragment_fill", Some(format)),
            RectPipeline::Copy(format) => ("fragment_copy", Some(format)),
            RectPipeline::FillDepth => ("fragment_nothing", None),
        };
        let targets = [format.map(|format| wgpu::ColorTargetState {
            format,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rectangle"),
            layout: Some(&self.rect_layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vertex_cover"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: format.is_none().then(|| wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some(fragment),
                compilation_options: Default::default(),
                targets: if format.is_some() { &targets } else { &[] },
            }),
            multiview_mask: None,
            cache: None,
        });
        self.rect_pipelines.insert(key, pipeline);
    }

    /// Draws one triangle over a rectangle of a surface with a fill or copy
    /// pipeline.
    fn draw_rect(
        &mut self,
        pipeline: RectPipeline,
        target: SurfaceId,
        area: Rect,
        uniforms: RectUniforms,
        source: Option<&wgpu::TextureView>,
    ) {
        self.rect_pipeline(pipeline);
        let offset = self.rect_uniforms(&uniforms);
        let source = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("copy source"),
            layout: &self.source_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source.unwrap_or(&self.white)),
            }],
        });
        let surface = self.surface(target);
        let keep = wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store };
        let color = [(pipeline != RectPipeline::FillDepth).then_some(wgpu::RenderPassColorAttachment {
            view: &surface.view,
            depth_slice: None,
            resolve_target: None,
            ops: keep,
        })];
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rectangle"),
                color_attachments: if pipeline == RectPipeline::FillDepth { &[] } else { &color },
                depth_stencil_attachment: (pipeline == RectPipeline::FillDepth).then_some(
                    wgpu::RenderPassDepthStencilAttachment {
                        view: &surface.view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    },
                ),
                ..Default::default()
            });
            pass.set_pipeline(&self.rect_pipelines[&pipeline]);
            pass.set_bind_group(0, &self.rect_group, &[offset]);
            pass.set_bind_group(1, &source, &[]);
            pass.set_viewport(area.x as f32, area.y as f32, area.width as f32, area.height as f32, 0.0, 1.0);
            pass.set_scissor_rect(area.x, area.y, area.width, area.height);
            pass.draw(0..3, 0..1);
        }
        self.finished.push(encoder.finish());
    }

    /// Draws the triangles collected so far.
    fn flush(&mut self) {
        let Some((target, depth)) = self.pending.target.take() else { return };
        let pending = std::mem::take(&mut self.pending);
        for command in &pending.commands {
            self.pipeline(command.pipeline);
            self.texture_group(command.texture);
        }
        let mut uniform_bytes = vec![0u8; pending.uniforms.len() * self.uniform_stride as usize];
        for (slot, uniforms) in uniform_bytes.chunks_mut(self.uniform_stride as usize).zip(&pending.uniforms)
        {
            slot[..std::mem::size_of::<DrawUniforms>()].copy_from_slice(bytemuck::bytes_of(uniforms));
        }
        // Index data is written in multiples of four bytes.
        let mut index_bytes = bytemuck::cast_slice::<u16, u8>(&pending.indices).to_vec();
        index_bytes.resize(index_bytes.len().next_multiple_of(4), 0);
        let vertex_bytes = bytemuck::cast_slice::<Vertex, u8>(&pending.vertices);
        // After what the pending work uses of each buffer; if one has no
        // room left, the pending work is given to the adapter first and
        // the buffers are free again (and may then be replaced by larger
        // ones).
        let fits = |buffer: &Growing, used: u64, bytes: usize| used + bytes as u64 <= buffer.buffer.size();
        if !(fits(&self.vertices, self.used.vertices, vertex_bytes.len())
            && fits(&self.indices, self.used.indices, index_bytes.len())
            && fits(&self.uniforms, self.used.uniforms, uniform_bytes.len()))
        {
            self.submit();
        }
        self.vertices.reserve(&self.device, vertex_bytes.len() as u64);
        self.indices.reserve(&self.device, index_bytes.len() as u64);
        if self.uniforms.reserve(&self.device, uniform_bytes.len() as u64) {
            self.uniform_group =
                Gpu::uniform_group(&self.device, &self.draw_uniform_layout, &self.uniforms.buffer);
        }
        let at = self.used;
        self.queue.write_buffer(&self.vertices.buffer, at.vertices, vertex_bytes);
        self.queue.write_buffer(&self.indices.buffer, at.indices, &index_bytes);
        self.queue.write_buffer(&self.uniforms.buffer, at.uniforms, &uniform_bytes);
        self.used.vertices += vertex_bytes.len() as u64;
        self.used.indices += index_bytes.len() as u64;
        self.used.uniforms += uniform_bytes.len() as u64;

        let keep = wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store };
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("triangles"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.surface(target).view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: keep,
                })],
                depth_stencil_attachment: depth.map(|depth| wgpu::RenderPassDepthStencilAttachment {
                    view: &self.surface(depth).view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_vertex_buffer(0, self.vertices.buffer.slice(at.vertices..));
            pass.set_index_buffer(self.indices.buffer.slice(at.indices..), wgpu::IndexFormat::Uint16);
            for (number, command) in pending.commands.iter().enumerate() {
                let Rect { x, y, width, height } = command.viewport;
                pass.set_pipeline(&self.pipelines[&command.pipeline]);
                let uniforms = at.uniforms + number as u64 * self.uniform_stride;
                pass.set_bind_group(0, &self.uniform_group, &[uniforms as u32]);
                pass.set_bind_group(1, &self.texture_groups[&command.texture], &[]);
                pass.set_viewport(x as f32, y as f32, width as f32, height as f32, 0.0, 1.0);
                pass.set_scissor_rect(x, y, width, height);
                pass.draw_indexed(command.indices.clone(), command.base_vertex, 0..1);
            }
        }
        self.finished.push(encoder.finish());
        self.statistics.passes += 1;
        // Keep the vectors' memory for the next pass.
        self.pending = Pending { target: None, ..pending };
        self.pending.vertices.clear();
        self.pending.indices.clear();
        self.pending.uniforms.clear();
        self.pending.commands.clear();
    }
}

impl Video for Gpu {
    fn create_surface(&mut self, width: u32, height: u32, format: PixelFormat) -> SurfaceId {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("surface"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: texture_format(format),
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let surface = Some(Surface { texture, view, width, height, format });
        match self.surfaces.iter().position(Option::is_none) {
            Some(index) => {
                self.surfaces[index] = surface;
                SurfaceId(index as u32)
            }
            None => {
                self.surfaces.push(surface);
                SurfaceId(self.surfaces.len() as u32 - 1)
            }
        }
    }

    fn destroy_surface(&mut self, surface: SurfaceId) {
        self.flush();
        self.submit();
        self.texture_groups.retain(|key, _| key.is_none_or(|(id, _)| id != surface));
        if self.presented == Some(surface) {
            self.presented = None;
        }
        self.surfaces[surface.0 as usize] = None;
    }

    fn upload(&mut self, id: SurfaceId, area: Rect, pitch: usize, pixels: &[u8]) {
        self.flush();
        // The pixels written below are there before any pending work
        // runs: what was drawn from the surface so far must run first.
        self.submit();
        let surface = self.surface(id);
        let size = surface.format.bytes_per_pixel() as usize;
        let row_bytes = area.width as usize * size;
        let mut texels = Vec::with_capacity((area.width * area.height * surface.texel_size()) as usize);
        for row in pixels.chunks(pitch).take(area.height as usize) {
            match surface.texture_format() {
                COLOR_FORMAT => {
                    for raw in row[..row_bytes].chunks(size) {
                        let raw = raw.iter().rev().fold(0, |value, &byte| value << 8 | byte as u32);
                        let argb = surface.format.to_argb(raw).expect("a colour format");
                        let [blue, green, red, alpha] = argb.to_le_bytes();
                        texels.extend_from_slice(&[red, green, blue, alpha]);
                    }
                }
                // Depth values and palette indices are stored as they are.
                _ => texels.extend_from_slice(&row[..row_bytes]),
            }
        }
        if surface.format == PixelFormat::Depth16 && !self.depth_copies {
            return self.depth_from_numbers(id, area, &texels);
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &surface.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: area.x, y: area.y, z: 0 },
                aspect: surface.aspect(),
            },
            &texels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(area.width * surface.texel_size()),
                rows_per_image: None,
            },
            wgpu::Extent3d { width: area.width, height: area.height, depth_or_array_layers: 1 },
        );
    }

    fn download(&mut self, id: SurfaceId, area: Rect, pitch: usize, pixels: &mut [u8]) {
        self.flush();
        let texels = self.read_texels(id, area);
        let surface = self.surface(id);
        let size = surface.format.bytes_per_pixel() as usize;
        let texel_row = (area.width * surface.texel_size()) as usize;
        for (row, texels) in pixels.chunks_mut(pitch).zip(texels.chunks(texel_row)) {
            match surface.texture_format() {
                COLOR_FORMAT => {
                    for (raw, texel) in row.chunks_mut(size).zip(texels.chunks(4)) {
                        let argb = u32::from_le_bytes([texel[2], texel[1], texel[0], texel[3]]);
                        raw.copy_from_slice(&surface.format.from_argb(argb).to_le_bytes()[..size]);
                    }
                }
                _ => row[..texel_row].copy_from_slice(texels),
            }
        }
    }

    fn fill(&mut self, id: SurfaceId, area: Rect, value: u32) {
        self.flush();
        let surface = self.surface(id);
        let area = surface.clamp(area);
        if area.width == 0 || area.height == 0 {
            return;
        }
        let (pipeline, color) = match surface.format {
            PixelFormat::Depth16 => {
                (RectPipeline::FillDepth, [(value & 0xFFFF) as f32 / 65535.0, 0.0, 0.0, 0.0])
            }
            format => (RectPipeline::Fill(surface.texture_format()), shader_color(format, value)),
        };
        let uniforms = RectUniforms { value: color, source: [0; 4], destination: [0; 4], keyed: [0; 4] };
        self.draw_rect(pipeline, id, area, uniforms, None);
    }

    fn copy(
        &mut self,
        source: SurfaceId,
        from: Rect,
        target: SurfaceId,
        to: Rect,
        color_key: Option<(u32, u32)>,
    ) {
        self.flush();
        let (from_surface, to_surface) = (self.surface(source), self.surface(target));
        assert_ne!(
            to_surface.format,
            PixelFormat::Depth16,
            "copies between depth surfaces are not implemented"
        );
        assert_eq!(from_surface.texture_format(), to_surface.texture_format(), "surfaces of one kind");
        // The key is one pixel value; DirectDraw's range keys have not been needed.
        let key = color_key.map(|(low, high)| {
            assert_eq!(low, high, "colour key ranges are not implemented");
            shader_color(from_surface.format, low)
        });
        let uniforms = RectUniforms {
            value: key.unwrap_or([0.0; 4]),
            source: [from.x, from.y, from.width, from.height],
            destination: [to.x, to.y, to.width, to.height],
            keyed: [key.is_some() as u32, 0, 0, 0],
        };
        let pipeline = RectPipeline::Copy(to_surface.texture_format());
        // A texture cannot be read while it is drawn to: a surface copied
        // onto itself is read from a copy of the source rectangle.
        let view = if source == target {
            let copy = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("copy of the source"),
                size: wgpu::Extent3d {
                    width: from_surface.width,
                    height: from_surface.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: from_surface.texture_format(),
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            let origin = wgpu::Origin3d { x: from.x, y: from.y, z: 0 };
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo { origin, ..from_surface.texture.as_image_copy() },
                wgpu::TexelCopyTextureInfo { origin, ..copy.as_image_copy() },
                wgpu::Extent3d { width: from.width, height: from.height, depth_or_array_layers: 1 },
            );
            self.finished.push(encoder.finish());
            copy.create_view(&wgpu::TextureViewDescriptor::default())
        } else {
            from_surface.view.clone()
        };
        self.draw_rect(pipeline, target, to, uniforms, Some(&view));
    }

    fn draw(&mut self, draw: &Draw) {
        let attachments = (draw.target, draw.depth);
        // One render pass has one set of attachments and no more vertices
        // than an index can reach from its batch's first.
        if self.pending.target.is_some_and(|pending| pending != attachments) {
            self.flush();
        }
        // A surface cannot be a texture of the pass that draws on it. The
        // program is not expected to do that; if it does, it reads what was
        // there before this pass.
        self.pending.target = Some(attachments);
        let DrawState { depth, blend, alpha_test, cull, flat, specular, fog, color, alpha, texture } =
            draw.state;
        let target = self.surface(draw.target);
        let viewport = target.clamp(draw.viewport);
        if viewport.width == 0 || viewport.height == 0 || viewport != draw.viewport {
            // The clip square maps to the viewport asked for; one that is
            // not wholly on the target cannot be set.
            assert_eq!(viewport, draw.viewport, "a viewport inside its target");
            return;
        }
        let key = texture.and_then(|sampling| sampling.color_key.map(|raw| (sampling.surface, raw)));
        let uniforms = DrawUniforms {
            color: combiner(color),
            alpha: combiner(alpha),
            test: [
                alpha_test.map_or(0, |(function, _)| alpha_function(function)),
                alpha_test.map_or(0, |(_, reference)| reference as u32),
                specular as u32,
                key.is_some() as u32,
            ],
            key: key.map_or([0.0; 4], |(surface, raw)| shader_color(self.surface(surface).format, raw)),
            fog: fog.map_or([0.0; 4], |color| shader_color(PixelFormat::Xrgb8888, color)),
            nearest: [
                texture.is_some_and(|sampling| {
                    sampling.magnify == Filter::Nearest && sampling.minify == Filter::Nearest
                }) as u32,
                0,
                0,
                0,
            ],
        };
        let command = Command {
            pipeline: PipelineKey { depth_attached: draw.depth.is_some(), depth, blend, cull },
            viewport,
            texture: texture.map(|sampling| {
                let key = SamplerKey {
                    magnify: sampling.magnify,
                    minify: sampling.minify,
                    address_u: sampling.address_u,
                    address_v: sampling.address_v,
                };
                (sampling.surface, key)
            }),
            indices: self.pending.indices.len() as u32
                ..(self.pending.indices.len() + draw.indices.len()) as u32,
            base_vertex: self.pending.vertices.len() as i32,
        };
        // A batch with the state of the one before it goes on in the same
        // draw call, if its vertices are still within an index's reach of
        // that call's first: the triangles are drawn in the same order
        // with the same state, in fewer calls.
        let added = if flat { draw.indices.len() } else { draw.vertices.len() };
        let joined = match (self.pending.commands.last(), self.pending.uniforms.last()) {
            (Some(last), Some(last_uniforms))
                if last.pipeline == command.pipeline
                    && last.viewport == command.viewport
                    && last.texture == command.texture
                    && bytemuck::bytes_of(last_uniforms) == bytemuck::bytes_of(&uniforms)
                    && last.indices.end == command.indices.start =>
            {
                let offset = self.pending.vertices.len() - last.base_vertex as usize;
                (offset + added <= u16::MAX as usize + 1).then_some(offset as u16)
            }
            _ => None,
        };
        let offset = joined.unwrap_or(0);
        if flat {
            // Every pixel of a triangle takes the colours of its first
            // vertex. Which vertex a graphics API takes for that differs, so
            // each triangle gets three vertices of its own with those
            // colours, and nothing is left to the API. (The runtime keeps a
            // batch well under the 65,536 vertices an index reaches.)
            for (number, triangle) in draw.indices.as_chunks::<3>().0.iter().enumerate() {
                let first = draw.vertices[triangle[0] as usize];
                for &index in triangle {
                    let vertex = draw.vertices[index as usize];
                    self.pending.vertices.push(Vertex {
                        diffuse: first.diffuse,
                        specular: first.specular,
                        ..vertex
                    });
                }
                let base = offset + 3 * number as u16;
                self.pending.indices.extend_from_slice(&[base, base + 1, base + 2]);
            }
        } else {
            self.pending.vertices.extend_from_slice(draw.vertices);
            self.pending.indices.extend(draw.indices.iter().map(|index| index + offset));
        }
        match (joined, self.pending.commands.last_mut()) {
            (Some(_), Some(last)) => last.indices.end = self.pending.indices.len() as u32,
            _ => {
                self.pending.uniforms.push(uniforms);
                self.pending.commands.push(command);
            }
        }
        self.statistics.batches += 1;
        self.statistics.triangles += draw.indices.len() as u64 / 3;
    }

    fn present(&mut self, surface: SurfaceId) {
        self.flush();
        self.submit();
        self.presented = Some(surface);
        self.statistics.frames_presented += 1;
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use andy_runtime::video::{Headless, Sampling};

    use super::*;

    fn gpu() -> Gpu {
        Gpu::new().expect("these tests need a graphics adapter; a software one (lavapipe) will do")
    }

    fn opaque() -> DrawState {
        let diffuse = Argument { source: Source::Diffuse, complement: false, alpha_replicate: false };
        let pass = Combine { operation: Operation::SelectFirst, first: diffuse, second: diffuse };
        DrawState {
            depth: None,
            blend: None,
            alpha_test: None,
            cull: Cull::None,
            flat: false,
            specular: false,
            fog: None,
            color: pass,
            alpha: pass,
            texture: None,
        }
    }

    /// A vertex at a screen position of an 8x8 target, as the runtime makes them.
    fn vertex(x: f32, y: f32, z: f32, diffuse: u32, uv: [f32; 2]) -> Vertex {
        let position = [(x + 0.5) / 8.0 * 2.0 - 1.0, 1.0 - (y + 0.5) / 8.0 * 2.0, z, 1.0];
        Vertex { position, diffuse, specular: 0, uv }
    }

    /// A rectangle from (left, top) to (right, bottom) as two triangles,
    /// clockwise on the screen.
    fn quad(left: f32, top: f32, right: f32, bottom: f32, z: f32, diffuse: u32) -> ([Vertex; 4], [u16; 6]) {
        let vertices = [
            vertex(left, top, z, diffuse, [0.0, 0.0]),
            vertex(right, top, z, diffuse, [1.0, 0.0]),
            vertex(right, bottom, z, diffuse, [1.0, 1.0]),
            vertex(left, bottom, z, diffuse, [0.0, 1.0]),
        ];
        (vertices, [0, 1, 2, 0, 2, 3])
    }

    /// The 8x8 colour surface as rows of `0xAARRGGBB`.
    fn pixels(gpu: &mut Gpu, surface: SurfaceId) -> Vec<u32> {
        gpu.read_rgba(surface).chunks(4).map(|p| u32::from_be_bytes([p[3], p[0], p[1], p[2]])).collect()
    }

    fn draw_quad(
        gpu: &mut Gpu,
        target: SurfaceId,
        depth: Option<SurfaceId>,
        state: DrawState,
        quad: ([Vertex; 4], [u16; 6]),
    ) {
        let viewport = Rect::whole(8, 8);
        gpu.draw(&Draw { target, depth, viewport, state, vertices: &quad.0, indices: &quad.1 });
    }

    #[test]
    fn surfaces_fill_copy_and_transfer_like_the_ones_in_memory() {
        let mut gpu = gpu();
        let mut memory = Headless::default();
        for format in
            [PixelFormat::Rgb565, PixelFormat::Argb1555, PixelFormat::Argb4444, PixelFormat::Argb8888]
        {
            let size = format.bytes_per_pixel() as usize;
            let pattern: Vec<u8> = (0..16 * 8 * size).map(|n| (n * 37 + n / 7) as u8).collect();
            let results: Vec<Vec<u8>> = [&mut gpu as &mut dyn Video, &mut memory]
                .into_iter()
                .map(|video| {
                    let a = video.create_surface(16, 8, format);
                    let b = video.create_surface(16, 8, format);
                    video.upload(a, Rect::whole(16, 8), 16 * size, &pattern);
                    video.fill(b, Rect::whole(16, 8), 0x1234_5678 & ((1u64 << (8 * size)) - 1) as u32);
                    video.fill(b, Rect { x: 3, y: 2, width: 5, height: 4 }, 0);
                    // A plain copy, one that enlarges, one that shrinks, one
                    // that leaves a colour out, and one within a surface.
                    video.copy(
                        a,
                        Rect { x: 1, y: 1, width: 4, height: 3 },
                        b,
                        Rect { x: 0, y: 0, width: 4, height: 3 },
                        None,
                    );
                    video.copy(
                        a,
                        Rect { x: 2, y: 0, width: 3, height: 2 },
                        b,
                        Rect { x: 8, y: 1, width: 7, height: 5 },
                        None,
                    );
                    video.copy(a, Rect::whole(16, 8), b, Rect { x: 4, y: 6, width: 5, height: 2 }, None);
                    let key =
                        u32::from_le_bytes(std::array::from_fn(
                            |n| if n < size { pattern[size + n] } else { 0 },
                        ));
                    video.copy(
                        a,
                        Rect::whole(4, 2),
                        b,
                        Rect { x: 12, y: 6, width: 4, height: 2 },
                        Some((key, key)),
                    );
                    video.copy(
                        b,
                        Rect { x: 0, y: 0, width: 6, height: 4 },
                        b,
                        Rect { x: 2, y: 2, width: 6, height: 4 },
                        None,
                    );
                    let mut out = vec![0; 16 * 8 * size + 8];
                    video.download(b, Rect::whole(16, 8), 16 * size, &mut out);
                    let mut part = vec![0; 20 * 3];
                    video.download(a, Rect { x: 5, y: 2, width: 4, height: 3 }, 20, &mut part);
                    out.extend_from_slice(&part);
                    video.destroy_surface(a);
                    video.destroy_surface(b);
                    out
                })
                .collect();
            assert_eq!(results[0], results[1], "{format:?}");
        }
    }

    #[test]
    fn depth_surfaces_hold_sixteen_bits() {
        let mut gpu = gpu();
        let depth = gpu.create_surface(8, 8, PixelFormat::Depth16);
        let values: Vec<u8> = (0..64u16).flat_map(|n| (n * 1021).to_le_bytes()).collect();
        gpu.upload(depth, Rect::whole(8, 8), 16, &values);
        gpu.fill(depth, Rect { x: 2, y: 1, width: 3, height: 2 }, 0xABCD);
        let mut out = vec![0; 128];
        gpu.download(depth, Rect::whole(8, 8), 16, &mut out);
        for (n, (got, was)) in out.chunks(2).zip(values.chunks(2)).enumerate() {
            let filled = (2..5).contains(&(n % 8)) && (1..3).contains(&(n / 8));
            assert_eq!(got, if filled { &[0xCD, 0xAB][..] } else { was }, "pixel {n}");
        }
    }

    #[test]
    fn a_rectangle_covers_the_pixels_whose_centres_it_contains() {
        let mut gpu = gpu();
        let target = gpu.create_surface(8, 8, PixelFormat::Xrgb8888);
        gpu.fill(target, Rect::whole(8, 8), 0);
        // Screen coordinates name pixel centres: 2 to 6 covers pixels 2, 3, 4, 5.
        draw_quad(&mut gpu, target, None, opaque(), quad(2.0, 1.0, 6.0, 4.0, 0.5, 0xFF10_2030));
        let image = pixels(&mut gpu, target);
        for (n, &pixel) in image.iter().enumerate() {
            let inside = (2..6).contains(&(n % 8)) && (1..4).contains(&(n / 8));
            assert_eq!(pixel, if inside { 0xFF10_2030 } else { 0xFF00_0000 }, "pixel {n}");
        }
        assert_eq!(gpu.statistics, Statistics { passes: 1, batches: 1, triangles: 2, frames_presented: 0 });
    }

    #[test]
    fn culling_goes_by_the_winding_on_the_screen() {
        let mut gpu = gpu();
        let target = gpu.create_surface(8, 8, PixelFormat::Xrgb8888);
        let clockwise = quad(0.0, 0.0, 8.0, 8.0, 0.5, 0xFFFF_FFFF);
        let counter = (clockwise.0, [0, 2, 1, 0, 3, 2]);
        for (cull, indices, drawn) in [
            (Cull::None, clockwise.1, true),
            (Cull::None, counter.1, true),
            (Cull::Clockwise, clockwise.1, false),
            (Cull::Clockwise, counter.1, true),
            (Cull::CounterClockwise, clockwise.1, true),
            (Cull::CounterClockwise, counter.1, false),
        ] {
            gpu.fill(target, Rect::whole(8, 8), 0);
            draw_quad(&mut gpu, target, None, DrawState { cull, ..opaque() }, (clockwise.0, indices));
            let expected = if drawn { 0xFFFF_FFFF } else { 0xFF00_0000 };
            assert!(pixels(&mut gpu, target).iter().all(|&pixel| pixel == expected), "{cull:?} {indices:?}");
        }
    }

    #[test]
    fn depth_is_compared_and_written_as_the_state_says() {
        let mut gpu = gpu();
        let target = gpu.create_surface(8, 8, PixelFormat::Xrgb8888);
        let depth = gpu.create_surface(8, 8, PixelFormat::Depth16);
        gpu.fill(target, Rect::whole(8, 8), 0);
        gpu.fill(depth, Rect::whole(8, 8), 0xFFFF);
        let tested = |write| DrawState {
            depth: Some(DepthState { compare: Compare::LessEqual, write, bias: 0 }),
            ..opaque()
        };
        // Near on the left half; far over everything is hidden behind it;
        // nearer still but without a depth test or write does not block
        // what comes after.
        draw_quad(&mut gpu, target, Some(depth), tested(true), quad(0.0, 0.0, 4.0, 8.0, 0.25, 0xFFFF_0000));
        draw_quad(&mut gpu, target, Some(depth), tested(true), quad(0.0, 0.0, 8.0, 8.0, 0.75, 0xFF00_FF00));
        draw_quad(&mut gpu, target, Some(depth), opaque(), quad(6.0, 0.0, 8.0, 8.0, 0.0, 0xFF00_00FF));
        draw_quad(&mut gpu, target, Some(depth), tested(false), quad(6.0, 0.0, 8.0, 4.0, 0.5, 0xFFFF_FFFF));
        let image = pixels(&mut gpu, target);
        for (n, &pixel) in image.iter().enumerate() {
            let expected = match (n % 8, n / 8) {
                (0..4, _) => 0xFFFF_0000,
                (4..6, _) => 0xFF00_FF00,
                (_, 0..4) => 0xFFFF_FFFF,
                _ => 0xFF00_00FF,
            };
            assert_eq!(pixel, expected, "pixel {n}");
        }
        let mut values = vec![0; 128];
        gpu.download(depth, Rect::whole(8, 8), 16, &mut values);
        let values: Vec<u16> = values.chunks(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
        assert_eq!((values[0], values[5]), (0x4000, 0xBFFF));
        // Four batches, one render pass.
        assert_eq!(gpu.statistics.passes, 1);
    }

    #[test]
    fn textures_are_combined_tested_keyed_and_blended() {
        let mut gpu = gpu();
        let target = gpu.create_surface(8, 8, PixelFormat::Xrgb8888);
        // A 2x2 texture: opaque red, half-transparent green, transparent
        // blue, opaque white.
        let texture = gpu.create_surface(2, 2, PixelFormat::Argb8888);
        let texels = [0xFFFF_0000u32, 0x8000_FF00, 0x0000_00FF, 0xFFFF_FFFF];
        gpu.upload(texture, Rect::whole(2, 2), 8, bytemuck::cast_slice(&texels));
        let sampling = Sampling {
            surface: texture,
            magnify: Filter::Nearest,
            minify: Filter::Nearest,
            address_u: Address::Clamp,
            address_v: Address::Clamp,
            color_key: None,
        };
        let from = |source| Argument { source, complement: false, alpha_replicate: false };
        let modulate = Combine {
            operation: Operation::Modulate,
            first: from(Source::Texture),
            second: from(Source::Current),
        };
        let textured = DrawState { color: modulate, alpha: modulate, texture: Some(sampling), ..opaque() };
        let corners = |image: &[u32]| [image[0], image[7], image[56], image[63]];
        let whole = |diffuse| quad(0.0, 0.0, 8.0, 8.0, 0.5, diffuse);

        // Modulated by a grey of one half, without blending.
        gpu.fill(target, Rect::whole(8, 8), 0xFF20_2020);
        draw_quad(&mut gpu, target, None, textured, whole(0xFF80_8080));
        assert_eq!(corners(&pixels(&mut gpu, target)), [0xFF80_0000, 0x8000_8000, 0x0000_0080, 0xFF80_8080]);

        // The alpha test leaves out what is not above one half.
        gpu.fill(target, Rect::whole(8, 8), 0xFF20_2020);
        let tested = DrawState { alpha_test: Some((Compare::Greater, 0x80)), ..textured };
        draw_quad(&mut gpu, target, None, tested, whole(0xFFFF_FFFF));
        assert_eq!(corners(&pixels(&mut gpu, target)), [0xFFFF_0000, 0xFF20_2020, 0xFF20_2020, 0xFFFF_FFFF]);

        // The colour key leaves out one texel value.
        gpu.fill(target, Rect::whole(8, 8), 0xFF20_2020);
        let keyed =
            DrawState { texture: Some(Sampling { color_key: Some(0xFFFF_FFFF), ..sampling }), ..textured };
        draw_quad(&mut gpu, target, None, keyed, whole(0xFFFF_FFFF));
        assert_eq!(corners(&pixels(&mut gpu, target)), [0xFFFF_0000, 0x8000_FF00, 0x0000_00FF, 0xFF20_2020]);

        // Blended over the background by the texture's alpha.
        gpu.fill(target, Rect::whole(8, 8), 0xFF20_2020);
        let blended = DrawState {
            blend: Some((BlendFactor::SourceAlpha, BlendFactor::InverseSourceAlpha)),
            ..textured
        };
        draw_quad(&mut gpu, target, None, blended, whole(0xFFFF_FFFF));
        let image = pixels(&mut gpu, target);
        assert_eq!(image[0] & 0xFF_FFFF, 0xFF_0000);
        assert_eq!(image[56] & 0xFF_FFFF, 0x20_2020);
        let [blue, green, red, _] = image[7].to_le_bytes();
        assert!(
            red.abs_diff(0x10) <= 1 && green.abs_diff(0x90) <= 1 && blue.abs_diff(0x10) <= 1,
            "{:#x}",
            image[7]
        );

        // Specular is added after the combiner; flat shading takes the
        // colours of each triangle's first vertex.
        gpu.fill(target, Rect::whole(8, 8), 0);
        let (mut vertices, indices) = quad(0.0, 0.0, 8.0, 8.0, 0.5, 0xFF40_4040);
        vertices[0].specular = 0x0000_30F0;
        vertices[1].diffuse = 0xFFFF_FFFF;
        let flat = DrawState { flat: true, specular: true, ..opaque() };
        draw_quad(&mut gpu, target, None, flat, (vertices, indices));
        assert!(pixels(&mut gpu, target).iter().all(|&pixel| pixel == 0xFF40_70FF));

        // Fog: the specular alpha says how much of the colour is kept.
        gpu.fill(target, Rect::whole(8, 8), 0);
        let (mut vertices, indices) = quad(0.0, 0.0, 8.0, 8.0, 0.5, 0xFFFF_0000);
        for vertex in &mut vertices {
            vertex.specular = 0x4000_0000;
        }
        let foggy = DrawState { fog: Some(0x0000_00FF), ..opaque() };
        draw_quad(&mut gpu, target, None, foggy, (vertices, indices));
        let [blue, green, red, _] = pixels(&mut gpu, target)[27].to_le_bytes();
        assert!(
            red.abs_diff(0x40) <= 1 && green == 0 && blue.abs_diff(0xBF) <= 1,
            "{red:#x} {green:#x} {blue:#x}"
        );
    }
}
