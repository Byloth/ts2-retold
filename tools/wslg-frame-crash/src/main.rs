//! Opens a window with winit's own frame (client-side decorations on
//! Wayland) and fills it with a colour, every frame or, with the argument
//! `once`, a single time.

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

struct Picture {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

#[derive(Default)]
struct App {
    window: Option<Arc<Window>>,
    picture: Option<Picture>,
    once: bool,
    frames: u64,
}

impl App {
    fn draw(&mut self) {
        let Some(picture) = &self.picture else { return };
        if self.once && self.frames > 0 {
            return;
        }
        let wgpu::CurrentSurfaceTexture::Success(frame) = picture.surface.get_current_texture() else { return };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = picture.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let shade = (self.frames % 120) as f64 / 120.0;
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.1, g: shade, b: 0.4, a: 1.0 }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        picture.queue.submit([encoder.finish()]);
        picture.queue.present(frame);
        self.frames += 1;
        if self.frames % 60 == 0 {
            println!("{} frames", self.frames);
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(event_loop.create_window(Window::default_attributes()).expect("a window"));
        let size = window.inner_size();
        let picture = pollster::block_on(async {
            let display = Box::new(event_loop.owned_display_handle());
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(display));
            let surface = instance.create_surface(window.clone()).expect("a surface");
            // Every adapter that can draw to the window; a real graphics
            // card before a software one, or the backend named in BACKEND.
            let wanted = std::env::var("BACKEND").unwrap_or_default().to_ascii_lowercase();
            let mut adapters = instance.enumerate_adapters(wgpu::Backends::all()).await;
            for adapter in &adapters {
                let info = adapter.get_info();
                println!(
                    "adapter: {:?} | {} | {:?} | draws to the window: {}",
                    info.backend,
                    info.name,
                    info.device_type,
                    adapter.is_surface_supported(&surface)
                );
            }
            adapters.retain(|adapter| adapter.is_surface_supported(&surface));
            adapters.sort_by_key(|adapter| {
                let info = adapter.get_info();
                let named = !wanted.is_empty() && format!("{:?}", info.backend).to_ascii_lowercase() == wanted;
                (!named, info.device_type == wgpu::DeviceType::Cpu)
            });
            let adapter = adapters.into_iter().next().expect("an adapter");
            println!("chosen: {:?} | {}", adapter.get_info().backend, adapter.get_info().name);
            let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor::default()).await.expect("a device");
            let configuration = surface.get_default_config(&adapter, size.width.max(1), size.height.max(1)).expect("a format");
            surface.configure(&device, &configuration);
            Picture { surface, device, queue }
        });
        (self.window, self.picture) = (Some(window), Some(picture));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if let WindowEvent::CloseRequested = event {
            event_loop.exit();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Poll);
        self.draw();
    }
}

fn main() {
    let mut app = App { once: std::env::args().nth(1).as_deref() == Some("once"), ..Default::default() };
    EventLoop::new().expect("a window system").run_app(&mut app).expect("the event loop");
}
