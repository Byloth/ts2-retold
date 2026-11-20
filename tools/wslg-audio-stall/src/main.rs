//! Plays silence to the PulseAudio server for sixteen seconds and counts how
//! often the server asks for more, second by second, while a window (no
//! frame, filled with one dim colour) appears on the screen.
//!
//! A healthy server asks about fifty times a second throughout. WSLg's
//! (1.0.79) drops to about once a second from the moment the window is on
//! the screen, for every program playing sound, and stays there.
//!
//! Arguments:
//! - none: the sound starts first, the window appears while it plays;
//! - `late`: the sound starts four seconds after the window, and is well;
//! - `recover`: as with none, and at five seconds the stream is paused and
//!   started again, after which the server is well;
//! - `silent`: the window alone, to see its effect on another program's
//!   sound.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pulseaudio::AsPlaybackSource;
use pulseaudio::protocol::stream::{BufferAttr, StreamFlags};
use pulseaudio::protocol::{ChannelMap, ChannelVolume, PlaybackStreamParams, SampleFormat, SampleSpec};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

const SECONDS: u64 = 16;
const RATE: u32 = 44_100;

/// When the server asked for sound, in milliseconds from the start.
type Requests = Arc<Mutex<Vec<u64>>>;

/// A stream of silence: stereo, 16 bits, 40 ms kept by the server.
fn stream(start: Instant, requests: Requests) -> (pulseaudio::Client, pulseaudio::PlaybackStream) {
    let client = pulseaudio::Client::from_env(c"wslg-audio-stall").expect("a PulseAudio server");
    let silence = move |bytes: &mut [u8]| {
        bytes.fill(0);
        requests.lock().unwrap().push(start.elapsed().as_millis() as u64);
        bytes.len()
    };
    let params = PlaybackStreamParams {
        sample_spec: SampleSpec { format: SampleFormat::S16Le, channels: 2, sample_rate: RATE },
        channel_map: ChannelMap::stereo(),
        cvolume: Some(ChannelVolume::norm(2)),
        buffer_attr: BufferAttr { target_length: RATE * 4 * 40 / 1000, ..Default::default() },
        flags: StreamFlags { adjust_latency: true, ..Default::default() },
        ..Default::default()
    };
    let stream = pollster::block_on(client.create_playback_stream(params, silence.as_playback_source()))
        .expect("a playback stream");
    (client, stream)
}

struct Picture {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

struct App {
    start: Instant,
    requests: Requests,
    /// When the sound starts, if it has not yet.
    sound_at: Option<Duration>,
    sound: Option<(pulseaudio::Client, pulseaudio::PlaybackStream)>,
    window: Option<Arc<Window>>,
    picture: Option<Picture>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attributes = Window::default_attributes().with_title("wslg-audio-stall").with_decorations(false);
        let window = Arc::new(event_loop.create_window(attributes).expect("a window"));
        let size = window.inner_size();
        let picture = pollster::block_on(async {
            let display = Box::new(event_loop.owned_display_handle());
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(display));
            let surface = instance.create_surface(window.clone()).expect("a surface");
            let mut adapters = instance.enumerate_adapters(wgpu::Backends::all()).await;
            adapters.retain(|adapter| adapter.is_surface_supported(&surface));
            let adapter = adapters.into_iter().next().expect("an adapter");
            println!("adapter: {:?} | {}", adapter.get_info().backend, adapter.get_info().name);
            let (device, queue) =
                adapter.request_device(&wgpu::DeviceDescriptor::default()).await.expect("a device");
            let configuration =
                surface.get_default_config(&adapter, size.width.max(1), size.height.max(1)).expect("a format");
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
        let elapsed = self.start.elapsed();
        if elapsed.as_secs() >= SECONDS {
            event_loop.exit();
        }
        if self.sound_at.is_some_and(|at| elapsed >= at) {
            self.sound_at = None;
            self.sound = Some(stream(self.start, self.requests.clone()));
        }
        let Some(picture) = &self.picture else { return };
        let wgpu::CurrentSurfaceTexture::Success(frame) = picture.surface.get_current_texture() else { return };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = picture.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.1, g: 0.15, b: 0.4, a: 1.0 }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        picture.queue.submit([encoder.finish()]);
        picture.queue.present(frame);
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let start = Instant::now();
    let requests = Requests::default();
    let sound_at = match mode.as_str() {
        "silent" => None,
        "late" => Some(Duration::from_secs(4)),
        _ => Some(Duration::ZERO),
    };
    let mut app = App { start, requests: requests.clone(), sound_at: None, sound: None, window: None, picture: None };
    match sound_at {
        // Before the window exists at all.
        Some(Duration::ZERO) => app.sound = Some(stream(start, requests.clone())),
        at => app.sound_at = at,
    }
    if let (Some((_, stream)), "recover") = (&app.sound, mode.as_str()) {
        let stream = stream.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(5));
            let asked = Instant::now();
            pollster::block_on(async {
                stream.cork().await.expect("the stream paused");
                stream.uncork().await.expect("the stream started again");
            });
            println!("paused and started again; the server took {} ms to do it", asked.elapsed().as_millis());
        });
    }
    EventLoop::new().expect("a window system").run_app(&mut app).expect("the event loop");
    let requests = requests.lock().unwrap();
    let each_second: Vec<usize> =
        (0..SECONDS).map(|second| requests.iter().filter(|&&at| at / 1000 == second).count()).collect();
    println!("requests for sound in each second: {each_second:?}");
}
