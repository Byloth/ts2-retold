//! `andy play`: the game in a window.
//!
//! The machine runs in short stretches between the window's events. Each
//! picture the game presents is shown, the keys held down in the window are
//! the keyboard the game reads, and the machine's clock is kept in step with
//! real time: it runs no faster, and while the game waits its clock goes no
//! further than the present (`Machine::clock_limit`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use andy_gpu::{Gpu, Presenter};
use andy_runtime::{Machine, Stop};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

/// Instructions the machine runs before the window is looked at again: two
/// milliseconds of its clock.
const STRETCH: u64 = 2 * andy_runtime::INSTRUCTIONS_PER_MS;
/// How long the machine may run before events are handled again.
const TURN: Duration = Duration::from_millis(8);

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

struct Play {
    machine: Machine,
    window: Option<Arc<Window>>,
    presenter: Option<Presenter>,
    /// Frames of the game shown so far.
    frames_shown: u64,
    /// When the machine's clock read `clock_start`.
    started: Instant,
    clock_start: u64,
    /// Why the machine stopped, once it has.
    stopped: Option<Stop>,
    error: Option<String>,
}

impl Play {
    fn gpu(&mut self) -> &mut Gpu {
        self.machine.video.as_any_mut().downcast_mut().expect("the window's backend is the GPU one")
    }

    /// Shows the surface the game last presented.
    fn show(&mut self) {
        let Some(mut presenter) = self.presenter.take() else { return };
        let gpu = self.gpu();
        if let Some(surface) = gpu.presented {
            presenter.show(gpu, surface);
        }
        self.presenter = Some(presenter);
    }
}

impl ApplicationHandler for Play {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Toy Story 2: Retold")
            .with_inner_size(LogicalSize::new(960.0, 720.0));
        let opened =
            event_loop.create_window(attributes).map_err(|error| error.to_string()).and_then(|window| {
                let window = Arc::new(window);
                let size = window.inner_size();
                let (gpu, presenter) = Gpu::for_window(window.clone(), size.width, size.height)
                    .map_err(|error| error.to_string())?;
                Ok((window, gpu, presenter))
            });
        match opened {
            Ok((window, gpu, presenter)) => {
                println!("graphics adapter       {}", gpu.adapter_name());
                self.machine.video = Box::new(gpu);
                (self.window, self.presenter) = (Some(window), Some(presenter));
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
            }
            WindowEvent::RedrawRequested => self.show(),
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(key) = event.physical_key else { return };
                if let Some(code) = key_name(key).and_then(andy_runtime::keys::code) {
                    self.machine.os.input.keys[code as usize] = event.state == ElementState::Pressed;
                }
            }
            // A window that loses the keyboard has no keys held.
            WindowEvent::Focused(false) => self.machine.os.input.keys = [false; 256],
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() || self.stopped.is_some() {
            return;
        }
        let turn = Instant::now();
        loop {
            // The machine's clock may go as far as real time has, counted
            // from the same starting point.
            let now = self.started.elapsed().as_millis() as u64;
            let clock = self.machine.clock() - self.clock_start;
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
            match self.machine.run(STRETCH) {
                Stop::Budget | Stop::Idle => {}
                stop => {
                    self.stopped = Some(stop);
                    event_loop.exit();
                    return;
                }
            }
            let frames = self.machine.os.ddraw.frames;
            if frames != self.frames_shown {
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
pub fn play(machine: Machine) -> Result<(Machine, Option<Stop>), String> {
    let event_loop = EventLoop::new().map_err(|error| format!("no window system: {error}"))?;
    let mut play = Play {
        machine,
        window: None,
        presenter: None,
        frames_shown: 0,
        started: Instant::now(),
        clock_start: 0,
        stopped: None,
        error: None,
    };
    event_loop.run_app(&mut play).map_err(|error| error.to_string())?;
    match play.error {
        Some(error) => Err(error),
        None => Ok((play.machine, play.stopped)),
    }
}
