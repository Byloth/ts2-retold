//! `andy play`: the game in a window.
//!
//! The machine runs in short stretches between the window's events. Each
//! picture the game presents is shown, the keys held down in the window are
//! the keyboard the game reads, and the machine's clock is kept in step with
//! real time: it runs no faster, and while the game waits its clock goes no
//! further than the present (`Machine::clock_limit`).
//!
//! The window draws its frame itself (`crate::frame`) and asks the window
//! system for none: the pointer on the bar moves the window, on its edges
//! resizes it, and the three buttons minimise, maximise and close it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use andy_gpu::{Bar, Gpu, Overlay, Presenter};
use andy_runtime::{Machine, Stop};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, CursorIcon, ResizeDirection, Window, WindowId};

use crate::frame::{Button, Edge, Frame, Hit};
use crate::gauge::{self, Reading};

/// Instructions the machine runs before the window is looked at again: two
/// milliseconds of its clock.
const STRETCH: u64 = 2 * andy_runtime::INSTRUCTIONS_PER_MS;
/// How often the gauge takes a new reading.
const READING_EVERY: Duration = Duration::from_millis(500);
/// How long the machine may run before events are handled again.
const TURN: Duration = Duration::from_millis(8);
/// The game's picture at the size the window opens with, and the smallest
/// it can be made, in logical pixels.
const OPENING: (f64, f64) = (960.0, 720.0);
const SMALLEST: (f64, f64) = (320.0, 240.0);
/// Two presses on the bar this close in time, and in logical pixels, are a
/// double click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const DOUBLE_CLICK_REACH: f64 = 4.0;

/// The size of the window whose frame leaves this much room for the picture.
fn window_size((width, height): (f64, f64)) -> LogicalSize<f64> {
    let frame = Frame::new("", 0, 0, 1.0);
    let border = frame.border() as f64;
    LogicalSize::new(width + 2.0 * border, height + frame.bar_height() as f64 + border)
}

/// The DirectInput name of a key of the window.
fn key_name(key: KeyCode) -> Option<&'static str> {
    use KeyCode::*;
    Some(match key {
        Escape => "ESCAPE",
        Digit1 => "1",
        Digit2 => "2",
        Digit3 => "3",
        Digit4 => "4",
        Digit5 => "5",
        Digit6 => "6",
        Digit7 => "7",
        Digit8 => "8",
        Digit9 => "9",
        Digit0 => "0",
        Minus => "MINUS",
        Equal => "EQUALS",
        Backspace => "BACK",
        Tab => "TAB",
        KeyQ => "Q",
        KeyW => "W",
        KeyE => "E",
        KeyR => "R",
        KeyT => "T",
        KeyY => "Y",
        KeyU => "U",
        KeyI => "I",
        KeyO => "O",
        KeyP => "P",
        BracketLeft => "LBRACKET",
        BracketRight => "RBRACKET",
        Enter => "RETURN",
        ControlLeft => "LCONTROL",
        KeyA => "A",
        KeyS => "S",
        KeyD => "D",
        KeyF => "F",
        KeyG => "G",
        KeyH => "H",
        KeyJ => "J",
        KeyK => "K",
        KeyL => "L",
        Semicolon => "SEMICOLON",
        Quote => "APOSTROPHE",
        Backquote => "GRAVE",
        ShiftLeft => "LSHIFT",
        Backslash => "BACKSLASH",
        KeyZ => "Z",
        KeyX => "X",
        KeyC => "C",
        KeyV => "V",
        KeyB => "B",
        KeyN => "N",
        KeyM => "M",
        Comma => "COMMA",
        Period => "PERIOD",
        Slash => "SLASH",
        ShiftRight => "RSHIFT",
        NumpadMultiply => "MULTIPLY",
        AltLeft => "LMENU",
        Space => "SPACE",
        CapsLock => "CAPITAL",
        F1 => "F1",
        F2 => "F2",
        F3 => "F3",
        F4 => "F4",
        F5 => "F5",
        F6 => "F6",
        F7 => "F7",
        F8 => "F8",
        F9 => "F9",
        F10 => "F10",
        F11 => "F11",
        F12 => "F12",
        Numpad7 => "NUMPAD7",
        Numpad8 => "NUMPAD8",
        Numpad9 => "NUMPAD9",
        NumpadSubtract => "SUBTRACT",
        Numpad4 => "NUMPAD4",
        Numpad5 => "NUMPAD5",
        Numpad6 => "NUMPAD6",
        NumpadAdd => "ADD",
        Numpad1 => "NUMPAD1",
        Numpad2 => "NUMPAD2",
        Numpad3 => "NUMPAD3",
        Numpad0 => "NUMPAD0",
        NumpadDecimal => "DECIMAL",
        NumpadEnter => "NUMPADENTER",
        ControlRight => "RCONTROL",
        NumpadDivide => "DIVIDE",
        AltRight => "RMENU",
        Home => "HOME",
        ArrowUp => "UP",
        PageUp => "PRIOR",
        ArrowLeft => "LEFT",
        ArrowRight => "RIGHT",
        End => "END",
        ArrowDown => "DOWN",
        PageDown => "NEXT",
        Insert => "INSERT",
        Delete => "DELETE",
        _ => return None,
    })
}

/// What the gauge reads from: what happened since its last reading.
struct Meter {
    since: Instant,
    /// Frames of the game shown.
    frames: u32,
    /// The time spent running the machine: the game's code and what the
    /// runtime does for it, drawing included. The rest went into waiting
    /// for the game's clock and into putting frames on the screen, which
    /// waits for the display.
    busy: Duration,
    reading: Reading,
}

impl Meter {
    /// A new reading, if it is time for one.
    fn read(&mut self) -> Option<Reading> {
        let elapsed = self.since.elapsed();
        if elapsed < READING_EVERY {
            return None;
        }
        let (seconds, busy) = (elapsed.as_secs_f64(), self.busy.as_secs_f64());
        let frames = self.frames as f64;
        self.reading = Reading {
            frames: (frames / seconds).round() as u32,
            frame_tenths: if self.frames == 0 { 0 } else { (10_000.0 * seconds / frames).round() as u32 },
            // What the same work would give if nothing were waited for.
            could: if busy > 0.0 { (frames / busy).round() as u32 } else { 0 },
        };
        (self.since, self.frames, self.busy) = (Instant::now(), 0, Duration::ZERO);
        Some(self.reading)
    }
}

struct Play {
    machine: Machine,
    window: Option<Arc<Window>>,
    presenter: Option<Presenter>,
    /// The window's frame as it should look, and as the presenter has it.
    frame: Option<Frame>,
    drawn: Option<Frame>,
    /// Where the pointer is in the window, in physical pixels.
    pointer: Option<(f64, f64)>,
    /// When and where the bar was last pressed.
    bar_pressed: Option<(Instant, (f64, f64))>,
    /// Frames of the game shown so far.
    frames_shown: u64,
    meter: Meter,
    /// Whether the gauge is shown (F9), and the reading it shows.
    gauge: Option<Reading>,
    /// Whether the game takes the mouse at all (a modification that uses
    /// it is on), and whether it has it now: the pointer is then hidden
    /// and held, and its movement and buttons are the game's.
    mouse_wanted: bool,
    mouse_held: bool,
    /// The joypad found when the game started, if any.
    pads: Option<crate::pad::Pads>,
    /// Movement not yet a whole count, sideways and down.
    mouse_left_over: (f64, f64),
    /// When the machine's clock read `clock_start`.
    started: Instant,
    clock_start: u64,
    /// The clock value at which the window closes by itself.
    until: Option<u64>,
    /// What the sound device lost, if there is one.
    losses: Option<crate::speaker::Losses>,
    /// Why the machine stopped, once it has.
    stopped: Option<Stop>,
    error: Option<String>,
}

impl Play {
    /// Opens the sound device, once. Without one the game is silent.
    fn open_sound(&mut self) {
        if self.losses.is_some() || self.machine.audio.is_some() {
            return;
        }
        match crate::speaker::Speaker::open() {
            Ok(speaker) => {
                self.losses = Some(speaker.losses());
                self.machine.audio = Some(Box::new(speaker));
            }
            Err(error) => println!("no sound: {error}"),
        }
    }

    fn gpu(&mut self) -> &mut Gpu {
        self.machine.video.as_any_mut().downcast_mut().expect("the window's backend is the GPU one")
    }

    /// Gives the presenter the gauge's picture as it now reads, or takes
    /// it away.
    fn lay_gauge(&mut self) {
        let Some(mut presenter) = self.presenter.take() else { return };
        let scale = self.window.as_ref().map_or(1.0, |window| window.scale_factor());
        let picture = self.gauge.map(|reading| gauge::render(reading, scale));
        let overlay = picture.as_ref().map(|picture| Overlay {
            rgba: &picture.rgba,
            width: picture.width,
            height: picture.height,
            margin: (gauge::MARGIN * scale).round() as u32,
        });
        presenter.set_overlay(self.gpu(), overlay);
        self.presenter = Some(presenter);
    }

    /// Shows the frame and, inside it, the surface the game last presented.
    fn show(&mut self) {
        let Some(mut presenter) = self.presenter.take() else { return };
        if self.frame != self.drawn {
            self.drawn = self.frame.clone();
            if let Some(frame) = self.drawn.clone() {
                let bar = frame.render();
                presenter.set_bar(
                    self.gpu(),
                    Bar {
                        rgba: &bar.rgba,
                        width: bar.width,
                        height: bar.height,
                        border: frame.border(),
                        border_color: frame.border_color(),
                    },
                );
            }
        }
        let gpu = self.gpu();
        let presented = gpu.presented;
        presenter.show(gpu, presented);
        self.presenter = Some(presenter);
    }

    /// Changes the frame, and has the window drawn again if it looks
    /// different now.
    fn change_frame(&mut self, change: impl FnOnce(&mut Frame, &Window)) {
        let (Some(frame), Some(window)) = (&mut self.frame, &self.window) else { return };
        change(frame, window);
        if self.frame != self.drawn {
            window.request_redraw();
        }
    }

    /// What the pointer is over.
    /// Gives the mouse to the game, or back to the desktop.
    fn hold_mouse(&mut self, held: bool) {
        if held == self.mouse_held {
            return;
        }
        let Some(window) = &self.window else { return };
        if held {
            // Held where it is where the window system can, else kept
            // inside the window.
            let grabbed = window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
            if grabbed.is_err() {
                return;
            }
        } else {
            let _ = window.set_cursor_grab(CursorGrabMode::None);
            let input = &mut self.machine.os.input;
            input.mouse_buttons = Default::default();
            input.mouse_motion = Default::default();
        }
        window.set_cursor_visible(!held);
        self.mouse_held = held;
        self.mouse_left_over = (0.0, 0.0);
    }

    fn under_pointer(&self) -> Hit {
        match (&self.frame, self.pointer) {
            (Some(frame), Some((x, y))) => frame.hit(x, y),
            _ => Hit::Content,
        }
    }

    /// The pointer has moved, or what is under it has changed: the button
    /// under it is lit, and over an edge it shows which way that edge pulls.
    fn pointer_moved(&mut self) {
        let hit = self.under_pointer();
        if let Some(window) = &self.window {
            window.set_cursor(match hit {
                Hit::Edge(Edge::North) => CursorIcon::NResize,
                Hit::Edge(Edge::South) => CursorIcon::SResize,
                Hit::Edge(Edge::East) => CursorIcon::EResize,
                Hit::Edge(Edge::West) => CursorIcon::WResize,
                Hit::Edge(Edge::NorthEast) => CursorIcon::NeResize,
                Hit::Edge(Edge::NorthWest) => CursorIcon::NwResize,
                Hit::Edge(Edge::SouthEast) => CursorIcon::SeResize,
                Hit::Edge(Edge::SouthWest) => CursorIcon::SwResize,
                Hit::Content | Hit::Bar | Hit::Button(_) => CursorIcon::Default,
            });
        }
        self.change_frame(|frame, _| {
            frame.hovered = match hit {
                Hit::Button(button) => Some(button),
                _ => None,
            }
        });
    }

    /// The left button went down on the frame.
    fn pressed(&mut self, hit: Hit) {
        let (Some(window), Some(pointer), Some(frame)) = (self.window.clone(), self.pointer, &self.frame)
        else {
            return;
        };
        // The window system moves and resizes the window from here on; if
        // it cannot, the window stays as it is.
        match hit {
            Hit::Content => {}
            Hit::Edge(edge) => {
                let _ = window.drag_resize_window(match edge {
                    Edge::North => ResizeDirection::North,
                    Edge::South => ResizeDirection::South,
                    Edge::East => ResizeDirection::East,
                    Edge::West => ResizeDirection::West,
                    Edge::NorthEast => ResizeDirection::NorthEast,
                    Edge::NorthWest => ResizeDirection::NorthWest,
                    Edge::SouthEast => ResizeDirection::SouthEast,
                    Edge::SouthWest => ResizeDirection::SouthWest,
                });
            }
            Hit::Bar => {
                let reach = DOUBLE_CLICK_REACH * frame.scale;
                let again = self.bar_pressed.take().is_some_and(|(when, (x, y))| {
                    when.elapsed() <= DOUBLE_CLICK
                        && (pointer.0 - x).abs() <= reach
                        && (pointer.1 - y).abs() <= reach
                });
                if again {
                    window.set_maximized(!window.is_maximized());
                } else {
                    self.bar_pressed = Some((Instant::now(), pointer));
                    let _ = window.drag_window();
                }
            }
            Hit::Button(button) => self.change_frame(|frame, _| frame.pressed = Some(button)),
        }
    }

    /// The left button came up: a button of the frame pressed and released
    /// does what it is for.
    fn released(&mut self, hit: Hit, event_loop: &ActiveEventLoop) {
        let held = self.frame.as_ref().and_then(|frame| frame.pressed);
        self.change_frame(|frame, _| frame.pressed = None);
        let (Some(window), Some(button)) = (&self.window, held) else { return };
        if hit != Hit::Button(button) {
            return;
        }
        match button {
            Button::Minimise => window.set_minimized(true),
            Button::Maximise => window.set_maximized(!window.is_maximized()),
            Button::Close => event_loop.exit(),
        }
    }
}

impl ApplicationHandler for Play {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        const TITLE: &str = "Toy Story 2: Retold";
        let attributes = Window::default_attributes()
            .with_title(TITLE)
            .with_decorations(false)
            .with_inner_size(window_size(OPENING))
            .with_min_inner_size(window_size(SMALLEST));
        let opened =
            event_loop.create_window(attributes).map_err(|error| error.to_string()).and_then(|window| {
                let window = Arc::new(window);
                let size = window.inner_size();
                let display = Box::new(event_loop.owned_display_handle());
                let (gpu, presenter) = Gpu::for_window(window.clone(), display, size.width, size.height)
                    .map_err(|error| error.to_string())?;
                Ok((window, gpu, presenter))
            });
        match opened {
            Ok((window, gpu, presenter)) => {
                println!("graphics adapter       {}", gpu.adapter_name());
                self.machine.video = Box::new(gpu);
                let size = window.inner_size();
                let mut frame = Frame::new(TITLE, size.width, size.height, window.scale_factor());
                (frame.active, frame.maximized) = (window.has_focus(), window.is_maximized());
                self.frame = Some(frame);
                (self.window, self.presenter) = (Some(window), Some(presenter));
                // The window has a picture from the start.
                self.lay_gauge();
                self.show();
                // And only now is there sound: what plays it is on the
                // screen first.
                self.open_sound();
                (self.started, self.clock_start) = (Instant::now(), self.machine.clock());
            }
            Err(error) => {
                self.error = Some(error);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(mut presenter) = self.presenter.take() {
                    presenter.resize(self.gpu(), size.width, size.height);
                    self.presenter = Some(presenter);
                }
                self.change_frame(|frame, window| {
                    (frame.width, frame.height) = (size.width, size.height);
                    frame.maximized = window.is_maximized();
                });
                self.pointer_moved();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.change_frame(|frame, _| frame.scale = scale_factor);
            }
            WindowEvent::RedrawRequested => self.show(),
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer = Some((position.x, position.y));
                self.pointer_moved();
            }
            WindowEvent::CursorLeft { .. } => {
                self.pointer = None;
                self.pointer_moved();
            }
            // While the game has the mouse, its buttons are the game's.
            WindowEvent::MouseInput { state, button, .. } if self.mouse_held => {
                let number = match button {
                    MouseButton::Left => Some(0),
                    MouseButton::Right => Some(1),
                    MouseButton::Middle => Some(2),
                    MouseButton::Back | MouseButton::Forward | MouseButton::Other(_) => Some(3),
                };
                if let Some(number) = number {
                    self.machine.os.input.mouse_buttons[number] = state == ElementState::Pressed;
                }
            }
            WindowEvent::MouseWheel { delta, .. } if self.mouse_held => {
                // A notch of the wheel is 120, as Windows counts it.
                let notches = match delta {
                    MouseScrollDelta::LineDelta(_, lines) => lines as f64,
                    MouseScrollDelta::PixelDelta(by) => by.y / 40.0,
                };
                self.machine.os.input.mouse_motion[2] += (notches * 120.0).round() as i32;
            }
            // A press on the picture gives the mouse to the game.
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. }
                if self.mouse_wanted && self.under_pointer() == Hit::Content =>
            {
                self.hold_mouse(true);
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                let hit = self.under_pointer();
                match state {
                    ElementState::Pressed => self.pressed(hit),
                    ElementState::Released => self.released(hit, event_loop),
                }
            }
            // The window system's menu for the window, where it has one.
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Right, .. } => {
                if let (Hit::Bar, Some(window), Some((x, y))) =
                    (self.under_pointer(), &self.window, self.pointer)
                {
                    window.show_window_menu(PhysicalPosition::new(x, y));
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(key) = event.physical_key else { return };
                // F9 is the window's own: it shows and hides the gauge,
                // and the game does not see it.
                // F10 too: it gives the pointer back to the desktop.
                if key == KeyCode::F10 {
                    if event.state == ElementState::Pressed {
                        self.hold_mouse(false);
                    }
                    return;
                }
                if key == KeyCode::F9 {
                    if event.state == ElementState::Pressed && !event.repeat {
                        self.gauge = if self.gauge.is_some() { None } else { Some(self.meter.reading) };
                        self.lay_gauge();
                        self.show();
                    }
                    return;
                }
                if let Some(code) = key_name(key).and_then(andy_runtime::keys::code) {
                    self.machine.os.input.keys[code as usize] = event.state == ElementState::Pressed;
                }
            }
            // A window that loses the keyboard has no keys held.
            WindowEvent::Focused(focused) => {
                if !focused {
                    self.machine.os.input.keys = [false; 256];
                    self.hold_mouse(false);
                }
                self.change_frame(|frame, _| frame.active = focused);
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        // The mouse's own movement, not the pointer's: it goes on when the
        // pointer is held in place.
        if let (true, DeviceEvent::MouseMotion { delta }) = (self.mouse_held, event) {
            let (x, y) = (self.mouse_left_over.0 + delta.0, self.mouse_left_over.1 + delta.1);
            let whole = (x.trunc(), y.trunc());
            self.mouse_left_over = (x - whole.0, y - whole.1);
            let motion = &mut self.machine.os.input.mouse_motion;
            motion[0] += whole.0 as i32;
            motion[1] += whole.1 as i32;
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() || self.stopped.is_some() {
            return;
        }
        self.advance(event_loop);
        // The gauge follows the meter while it is shown.
        if let Some(reading) = self.meter.read()
            && self.gauge.is_some_and(|shown| shown != reading)
        {
            self.gauge = Some(reading);
            self.lay_gauge();
            self.show();
        }
    }
}

impl Play {
    /// Lets the machine run as far as real time has, showing its frames.
    fn advance(&mut self, event_loop: &ActiveEventLoop) {
        let turn = Instant::now();
        loop {
            // The machine's clock may go as far as real time has, counted
            // from the same starting point.
            let now = self.started.elapsed().as_millis() as u64;
            let clock = self.machine.clock() - self.clock_start;
            if self.until.is_some_and(|until| self.machine.clock() >= until) {
                event_loop.exit();
                return;
            }
            // Far behind (the host was busy, or is too slow): do not try to
            // catch up, take the present as the new starting point.
            if now.saturating_sub(clock) > 250 {
                (self.started, self.clock_start) = (Instant::now(), self.machine.clock());
                continue;
            }
            // Level with real time: come back in a millisecond.
            if clock >= now {
                let next = self.started + Duration::from_millis(clock + 1);
                event_loop.set_control_flow(ControlFlow::WaitUntil(next));
                return;
            }
            self.machine.clock_limit = Some(self.clock_start + now);
            if let Some(pads) = &mut self.pads {
                self.machine.os.input.pad = Some(pads.read());
            }
            let running = Instant::now();
            let stop = self.machine.run(STRETCH);
            self.meter.busy += running.elapsed();
            match stop {
                Stop::Budget | Stop::Idle => {}
                stop => {
                    self.stopped = Some(stop);
                    event_loop.exit();
                    return;
                }
            }
            let frames = self.machine.os.ddraw.frames;
            if frames != self.frames_shown {
                self.meter.frames += (frames - self.frames_shown) as u32;
                self.frames_shown = frames;
                self.show();
            }
            if turn.elapsed() >= TURN {
                event_loop.set_control_flow(ControlFlow::Poll);
                return;
            }
        }
    }
}

/// Runs the game in a window until it stops or the window is closed.
pub fn play(machine: Machine, seconds: Option<u64>, gauge: bool) -> Result<(Machine, Option<Stop>), String> {
    let event_loop = EventLoop::new().map_err(|error| format!("no window system: {error}"))?;
    let mods = &machine.os.mods;
    let mouse_wanted = [andy_runtime::mods::MOUSE_CONTROLS, andy_runtime::mods::ORBIT_CAMERA]
        .iter()
        .any(|id| mods.is_enabled(id));
    // The game looks for joysticks once, at its start: a pad plugged in
    // now is its joystick for the session.
    let mut machine = machine;
    let mut pads = crate::pad::Pads::open();
    if let Some(pads) = &mut pads {
        println!("joypad                 {}", pads.name());
        machine.os.input.pad = Some(pads.read());
    }
    // A recording starts from the input there is before the game does.
    if machine.os.recorder.is_some() {
        machine.os.recorder = Some(andy_runtime::input::Recorder::starting_with(&machine.os.input));
    }
    let mut play = Play {
        machine,
        window: None,
        presenter: None,
        frame: None,
        drawn: None,
        pointer: None,
        bar_pressed: None,
        frames_shown: 0,
        meter: Meter { since: Instant::now(), frames: 0, busy: Duration::ZERO, reading: Reading::default() },
        gauge: gauge.then(Reading::default),
        mouse_wanted,
        mouse_held: false,
        pads,
        mouse_left_over: (0.0, 0.0),
        started: Instant::now(),
        clock_start: 0,
        until: seconds.map(|seconds| seconds * 1000),
        losses: None,
        stopped: None,
        error: None,
    };
    event_loop.run_app(&mut play).map_err(|error| error.to_string())?;
    // The sound device is let go before the machine is handed back.
    play.machine.audio = None;
    let reading = play.meter.reading;
    println!(
        "at the end             {} frames a second, {:.1} ms a frame; {} a second without waiting",
        reading.frames,
        reading.frame_tenths as f64 / 10.0,
        reading.could
    );
    if let Some(losses) = play.losses.take() {
        let (starved, dropped) = losses.count();
        let rate = andy_runtime::audio::OUTPUT_RATE as u64;
        println!(
            "sound                  {} ms of silence for lack of sound, {} ms dropped for being late",
            starved * 1000 / rate,
            dropped * 1000 / rate
        );
    }
    match play.error {
        Some(error) => Err(error),
        None => Ok((play.machine, play.stopped)),
    }
}
