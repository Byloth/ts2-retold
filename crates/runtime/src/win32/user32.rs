//! USER32: windows, messages and dialogs.
//!
//! A small window system with the semantics the program can observe: window
//! classes, windows with a procedure in the program, a message queue, and the
//! messages Windows sends synchronously when a window is created, shown,
//! painted or destroyed. Nothing is drawn here; presentation belongs to the
//! display code.

use std::collections::VecDeque;

use andy_recomp::pe::{ResourceId, resource_type};

use super::{HostResult, functions};
use crate::{Machine, Stop};

pub const WM_CREATE: u32 = 0x0001;
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_MOVE: u32 = 0x0003;
pub const WM_SIZE: u32 = 0x0005;
pub const WM_ACTIVATE: u32 = 0x0006;
pub const WM_SETFOCUS: u32 = 0x0007;
pub const WM_KILLFOCUS: u32 = 0x0008;
pub const WM_PAINT: u32 = 0x000F;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_QUIT: u32 = 0x0012;
pub const WM_ERASEBKGND: u32 = 0x0014;
pub const WM_SHOWWINDOW: u32 = 0x0018;
pub const WM_ACTIVATEAPP: u32 = 0x001C;
pub const WM_GETMINMAXINFO: u32 = 0x0024;
pub const WM_WINDOWPOSCHANGING: u32 = 0x0046;
pub const WM_WINDOWPOSCHANGED: u32 = 0x0047;
pub const WM_NCCREATE: u32 = 0x0081;
pub const WM_NCDESTROY: u32 = 0x0082;
pub const WM_NCCALCSIZE: u32 = 0x0083;
pub const WM_NCACTIVATE: u32 = 0x0086;
pub const WM_QUERYENDSESSION: u32 = 0x0011;

const WS_VISIBLE: u32 = 0x1000_0000;
const CW_USEDEFAULT: u32 = 0x8000_0000;
const FIRST_WINDOW: u32 = 0x0001_0000;
/// Handles of loaded resources: tag in the top byte, index below.
const ICON_TAG: u32 = 0x0200_0000;
const CURSOR_TAG: u32 = 0x0300_0000;
/// Smallest window Windows lets the user drag to, in Windows 98 metrics.
const MIN_TRACK: (i32, i32) = (112, 27);

#[derive(Debug, Clone)]
struct Class {
    /// Upper-cased: class names compare without regard to case.
    name: Vec<u8>,
    procedure: u32,
    background: u32,
}

#[derive(Debug, Clone)]
pub struct Window {
    pub handle: u32,
    class: usize,
    pub procedure: u32,
    pub title: Vec<u8>,
    pub style: u32,
    pub ex_style: u32,
    /// Outer rectangle in screen coordinates: position and size.
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub parent: u32,
    pub menu: u32,
    pub instance: u32,
    /// Thickness of the border, caption and menu around the client area.
    frame: Frame,
    pub user_data: u32,
    pub visible: bool,
    alive: bool,
    needs_paint: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Message {
    pub window: u32,
    pub message: u32,
    pub wparam: u32,
    pub lparam: u32,
    pub time: u32,
}

/// The desktop the program starts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Display {
    pub width: u32,
    pub height: u32,
    pub bits_per_pixel: u32,
}

impl Default for Display {
    fn default() -> Self {
        // A common desktop of the game's time.
        Display {
            width: 1024,
            height: 768,
            bits_per_pixel: 16,
        }
    }
}

#[derive(Debug, Default)]
pub struct User {
    classes: Vec<Class>,
    pub windows: Vec<Window>,
    pub queue: VecDeque<Message>,
    /// Exit code given to `PostQuitMessage`, until `WM_QUIT` is retrieved.
    quit: Option<u32>,
    /// Window that is active and has the keyboard focus, or 0.
    pub active: u32,
    /// `ShowCursor` display count; the cursor shows while it is >= 0.
    pub cursor_count: i32,
    pub display: Display,
}

impl User {
    fn window(&self, handle: u32) -> Option<&Window> {
        let index = handle.checked_sub(FIRST_WINDOW)? / 4;
        self.windows
            .get(index as usize)
            .filter(|w| w.alive && w.handle == handle)
    }

    fn window_mut(&mut self, handle: u32) -> Option<&mut Window> {
        let index = handle.checked_sub(FIRST_WINDOW)? / 4;
        self.windows
            .get_mut(index as usize)
            .filter(|w| w.alive && w.handle == handle)
    }

    /// Queues a message, as `PostMessage` does.
    pub fn post(&mut self, window: u32, message: u32, wparam: u32, lparam: u32, time: u32) {
        self.queue.push_back(Message {
            window,
            message,
            wparam,
            lparam,
            time,
        });
    }
}

/// Space taken by a window's non-client area on each side.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Frame {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Frame {
    /// The non-client area Windows 98 gives a window of this style, with its
    /// default metrics: the rule behind `AdjustWindowRectEx`.
    pub fn of(style: u32, ex_style: u32, has_menu: bool) -> Frame {
        const WS_BORDER: u32 = 0x0080_0000;
        const WS_DLGFRAME: u32 = 0x0040_0000;
        const WS_CAPTION: u32 = WS_BORDER | WS_DLGFRAME;
        const WS_THICKFRAME: u32 = 0x0004_0000;
        const WS_EX_DLGMODALFRAME: u32 = 0x0001;
        const WS_EX_TOOLWINDOW: u32 = 0x0080;
        const WS_EX_CLIENTEDGE: u32 = 0x0200;
        const WS_EX_STATICEDGE: u32 = 0x0002_0000;
        const SIZING_FRAME: i32 = 4; // SM_CXFRAME
        const FIXED_FRAME: i32 = 3; // SM_CXDLGFRAME
        const CAPTION: i32 = 19; // SM_CYCAPTION
        const SMALL_CAPTION: i32 = 16; // SM_CYSMCAPTION
        const MENU: i32 = 19; // SM_CYMENU
        const EDGE: i32 = 2; // SM_CXEDGE

        let modal = ex_style & WS_EX_DLGMODALFRAME != 0;
        let mut border = if ex_style & (WS_EX_STATICEDGE | WS_EX_DLGMODALFRAME) == WS_EX_STATICEDGE {
            1
        } else if modal || style & (WS_THICKFRAME | WS_DLGFRAME) != 0 {
            2
        } else {
            0
        };
        if style & WS_THICKFRAME != 0 {
            border += SIZING_FRAME - FIXED_FRAME;
        }
        if style & (WS_BORDER | WS_DLGFRAME) != 0 || modal {
            border += 1;
        }
        if ex_style & WS_EX_CLIENTEDGE != 0 {
            border += EDGE;
        }
        let mut top = border;
        if style & WS_CAPTION == WS_CAPTION {
            top += if ex_style & WS_EX_TOOLWINDOW != 0 {
                SMALL_CAPTION
            } else {
                CAPTION
            };
        }
        if has_menu {
            top += MENU;
        }
        Frame {
            left: border,
            top,
            right: border,
            bottom: border,
        }
    }
}

impl Window {
    /// Top-left corner of the client area, in screen coordinates.
    pub fn client_origin(&self) -> (i32, i32) {
        (self.x + self.frame.left, self.y + self.frame.top)
    }

    pub fn client_size(&self) -> (i32, i32) {
        (
            (self.width - self.frame.left - self.frame.right).max(0),
            (self.height - self.frame.top - self.frame.bottom).max(0),
        )
    }
}

fn pack(low: i32, high: i32) -> u32 {
    (low as u32 & 0xFFFF) | (high as u32) << 16
}

fn not_implemented(what: String) -> HostResult {
    Err(Stop::Unimplemented {
        dll: "USER32.dll".into(),
        name: what,
    })
}

/// A resource name argument: a small integer or a pointer to a string.
fn resource_id(m: &mut Machine, value: u32) -> ResourceId {
    if value < 0x1_0000 {
        ResourceId::Id(value as u16)
    } else {
        ResourceId::Name(m.text(value))
    }
}

/// Index of a resource of the given type, if the executable has it.
fn find_resource(m: &mut Machine, kind: u16, name: u32) -> Option<usize> {
    let name = resource_id(m, name);
    m.os.resources
        .iter()
        .position(|r| r.kind.matches(&ResourceId::Id(kind)) && r.name.matches(&name))
}

impl Machine {
    /// Calls a window's procedure, as `SendMessage` does.
    pub fn send_message(&mut self, window: u32, message: u32, wparam: u32, lparam: u32) -> HostResult {
        match self.os.user.window(window).map(|w| w.procedure) {
            Some(procedure) => self.call_guest(procedure, &[window, message, wparam, lparam]),
            None => Ok(0),
        }
    }

    /// Sends a message whose `lparam` points at a structure, which lives on
    /// the program's stack for the duration of the call. Returns the result
    /// and the structure as the procedure left it.
    fn send_with_struct<const N: usize>(
        &mut self,
        window: u32,
        message: u32,
        wparam: u32,
        words: [u32; N],
    ) -> Result<(u32, [u32; N]), Stop> {
        let saved = self.cpu.regs[4];
        let at = (saved - 4 * N as u32) & !3;
        self.write_words(at, &words);
        self.cpu.regs[4] = at;
        let result = self.send_message(window, message, wparam, at);
        self.cpu.regs[4] = saved;
        let after = self.read_words(at);
        Ok((result?, after))
    }

    fn register_class(&mut self, words: &[u32]) -> u32 {
        // WNDCLASSA: style, procedure, class extra, window extra, instance,
        // icon, cursor, background, menu name, class name.
        let name = self.bytes(words[9]).to_ascii_uppercase();
        if name.is_empty() || self.os.user.classes.iter().any(|c| c.name == name) {
            return 0;
        }
        self.note(|| {
            format!(
                "class {:?}, procedure {:#x}",
                String::from_utf8_lossy(&name),
                words[1]
            )
        });
        self.os.user.classes.push(Class {
            name,
            procedure: words[1],
            background: words[7],
        });
        // Class atoms start at 0xC000.
        0xC000 + self.os.user.classes.len() as u32 - 1
    }

    fn create_window(&mut self, a: &[u32]) -> HostResult {
        let [
            ex_style,
            class_name,
            title,
            style,
            x,
            y,
            width,
            height,
            parent,
            menu,
            instance,
            param,
        ] = *<&[u32; 12]>::try_from(a).expect("CreateWindowExA takes twelve arguments");
        let class = if class_name < 0x1_0000 {
            class_name
                .checked_sub(0xC000)
                .map(|n| n as usize)
                .filter(|&n| n < self.os.user.classes.len())
        } else {
            let wanted = self.bytes(class_name).to_ascii_uppercase();
            self.os.user.classes.iter().position(|c| c.name == wanted)
        };
        let Some(class) = class else { return Ok(0) };
        // CW_USEDEFAULT lets Windows choose. A pop-up gets the origin and no
        // size; an overlapped window gets a placement that depends on what
        // else is on the desktop, here three quarters of the screen.
        const WS_POPUP: u32 = 0x8000_0000;
        const WS_CHILD: u32 = 0x4000_0000;
        let display = self.os.user.display;
        let overlapped = style & (WS_POPUP | WS_CHILD) == 0;
        let choose = |value: u32, chosen: u32| {
            if value == CW_USEDEFAULT {
                if overlapped { chosen as i32 } else { 0 }
            } else {
                value as i32
            }
        };
        let (x, y) = (choose(x, display.width / 8), choose(y, display.height / 8));
        let (width, height) = (
            choose(width, display.width * 3 / 4),
            choose(height, display.height * 3 / 4),
        );
        let handle = FIRST_WINDOW + 4 * self.os.user.windows.len() as u32;
        let window = Window {
            handle,
            class,
            procedure: self.os.user.classes[class].procedure,
            title: if title == 0 { Vec::new() } else { self.bytes(title) },
            style: style & !WS_VISIBLE,
            ex_style,
            x,
            y,
            width,
            height,
            parent,
            menu,
            instance,
            user_data: 0,
            visible: false,
            alive: true,
            needs_paint: false,
            frame: Frame::of(style, ex_style, menu != 0),
        };
        self.note(|| {
            format!(
                "{:?}, {}x{} at {},{}, style {style:#x}",
                String::from_utf8_lossy(&window.title),
                window.width,
                window.height,
                window.x,
                window.y
            )
        });
        let (wx, wy, ww, wh) = (window.x, window.y, window.width, window.height);
        self.os.user.windows.push(window);

        // The messages Windows sends before CreateWindowEx returns.
        let min_max = [
            0,
            0,
            display.width,
            display.height,
            0,
            0,
            MIN_TRACK.0 as u32,
            MIN_TRACK.1 as u32,
            display.width,
            display.height,
        ];
        self.send_with_struct(handle, WM_GETMINMAXINFO, 0, min_max)?;
        // CREATESTRUCTA, fields in reverse order of the arguments.
        let create = [
            param, instance, menu, parent, wh as u32, ww as u32, wy as u32, wx as u32, style, title,
            class_name, ex_style,
        ];
        let (accepted, _) = self.send_with_struct(handle, WM_NCCREATE, 0, create)?;
        if accepted == 0 {
            self.destroy_window(handle, false)?;
            return Ok(0);
        }
        let rect = [wx as u32, wy as u32, (wx + ww) as u32, (wy + wh) as u32];
        self.send_with_struct(handle, WM_NCCALCSIZE, 0, rect)?;
        let (created, _) = self.send_with_struct(handle, WM_CREATE, 0, create)?;
        if created == u32::MAX {
            self.destroy_window(handle, true)?;
            return Ok(0);
        }
        if style & WS_VISIBLE != 0 {
            self.show_window(handle, true)?;
        }
        Ok(handle)
    }

    fn destroy_window(&mut self, handle: u32, notify: bool) -> Result<bool, Stop> {
        if self.os.user.window(handle).is_none() {
            return Ok(false);
        }
        if notify {
            self.send_message(handle, WM_DESTROY, 0, 0)?;
        }
        self.send_message(handle, WM_NCDESTROY, 0, 0)?;
        if let Some(window) = self.os.user.window_mut(handle) {
            window.alive = false;
            window.visible = false;
        }
        if self.os.user.active == handle {
            self.os.user.active = 0;
        }
        self.os.user.queue.retain(|message| message.window != handle);
        Ok(true)
    }

    /// Shows or hides a window, with the messages Windows sends for it.
    /// Returns whether the window was visible before.
    fn show_window(&mut self, handle: u32, show: bool) -> Result<bool, Stop> {
        let Some(window) = self.os.user.window(handle) else {
            return Ok(false);
        };
        let was_visible = window.visible;
        let (x, y, width, height) = (window.x, window.y, window.width, window.height);
        if show == was_visible {
            return Ok(was_visible);
        }
        self.send_message(handle, WM_SHOWWINDOW, show as u32, 0)?;
        // WINDOWPOS: window, insert-after, x, y, cx, cy, flags.
        const SWP_NOSIZE_NOMOVE: u32 = 0x0003;
        const SWP_SHOWWINDOW: u32 = 0x0040;
        const SWP_HIDEWINDOW: u32 = 0x0080;
        let flags = SWP_NOSIZE_NOMOVE | if show { SWP_SHOWWINDOW } else { SWP_HIDEWINDOW };
        let position = [handle, 0, x as u32, y as u32, width as u32, height as u32, flags];
        self.send_with_struct(handle, WM_WINDOWPOSCHANGING, 0, position)?;
        if let Some(window) = self.os.user.window_mut(handle) {
            window.visible = show;
            window.needs_paint = show;
        }
        if show {
            self.activate(handle)?;
        } else if self.os.user.active == handle {
            self.deactivate()?;
        }
        self.send_with_struct(handle, WM_WINDOWPOSCHANGED, 0, position)?;
        if show {
            // A window learns its size and position the first time it is shown.
            if let Some(window) = self.os.user.window(handle) {
                let ((cx, cy), (cw, ch)) = (window.client_origin(), window.client_size());
                self.send_message(handle, WM_SIZE, 0, pack(cw, ch))?;
                self.send_message(handle, WM_MOVE, 0, pack(cx, cy))?;
            }
        }
        Ok(was_visible)
    }

    fn deactivate(&mut self) -> Result<(), Stop> {
        let old = std::mem::take(&mut self.os.user.active);
        if old != 0 {
            self.send_message(old, WM_NCACTIVATE, 0, 0)?;
            self.send_message(old, WM_ACTIVATE, 0, 0)?;
            self.send_message(old, WM_KILLFOCUS, 0, 0)?;
        }
        Ok(())
    }

    /// Makes a window the active one and gives it the keyboard focus.
    fn activate(&mut self, handle: u32) -> Result<(), Stop> {
        if self.os.user.active == handle {
            return Ok(());
        }
        let old = self.os.user.active;
        if old == 0 {
            // The program's first active window: the application itself
            // becomes active. There is never another application here.
            self.send_message(handle, WM_ACTIVATEAPP, 1, 0)?;
        } else {
            self.deactivate()?;
        }
        self.os.user.active = handle;
        const WA_ACTIVE: u32 = 1;
        self.send_message(handle, WM_NCACTIVATE, 1, 0)?;
        self.send_message(handle, WM_ACTIVATE, WA_ACTIVE, old)?;
        self.send_message(handle, WM_SETFOCUS, old, 0)?;
        Ok(())
    }

    /// Takes the next message for the program, if any: queued messages
    /// first, then `WM_QUIT`, then a paint request for a window that needs it.
    fn next_message(&mut self, remove: bool) -> Option<Message> {
        let user = &mut self.os.user;
        if let Some(&message) = user.queue.front() {
            if remove {
                user.queue.pop_front();
            }
            return Some(message);
        }
        if let Some(code) = user.quit {
            if remove {
                user.quit = None;
            }
            return Some(Message {
                window: 0,
                message: WM_QUIT,
                wparam: code,
                lparam: 0,
                time: self.os.now_ms,
            });
        }
        let window = user
            .windows
            .iter()
            .find(|w| w.alive && w.visible && w.needs_paint)?;
        Some(Message {
            window: window.handle,
            message: WM_PAINT,
            wparam: 0,
            lparam: 0,
            time: self.os.now_ms,
        })
    }

    fn write_message(&mut self, address: u32, message: &Message) {
        // MSG: window, message, wParam, lParam, time, cursor x, cursor y.
        self.write_words(
            address,
            &[
                message.window,
                message.message,
                message.wparam,
                message.lparam,
                message.time,
                0,
                0,
            ],
        );
    }

    fn default_window_procedure(&mut self, a: &[u32]) -> HostResult {
        let (window, message) = (a[0], a[1]);
        Ok(match message {
            WM_NCCREATE | WM_NCACTIVATE | WM_QUERYENDSESSION => 1,
            WM_NCCALCSIZE => {
                // Shrinks the window rectangle at lParam to the client area.
                if let Some(frame) = self.os.user.window(window).map(|w| w.frame) {
                    let [left, top, right, bottom]: [u32; 4] = self.read_words(a[3]);
                    let rect = [
                        left.wrapping_add(frame.left as u32),
                        top.wrapping_add(frame.top as u32),
                        right.wrapping_sub(frame.right as u32),
                        bottom.wrapping_sub(frame.bottom as u32),
                    ];
                    self.write_words(a[3], &rect);
                }
                0
            }
            WM_CLOSE => {
                self.destroy_window(window, true)?;
                0
            }
            WM_PAINT => {
                // An unhandled paint request is simply validated.
                if let Some(window) = self.os.user.window_mut(window) {
                    window.needs_paint = false;
                }
                0
            }
            // Erased with the class brush: report it done when there is one.
            WM_ERASEBKGND => {
                let class = self.os.user.window(window).map(|w| w.class);
                class.is_some_and(|class| self.os.user.classes[class].background != 0) as u32
            }
            _ => 0,
        })
    }
}

functions! { "USER32.dll";
    MessageBoxA(4) => |m, a| {
        let (text, caption) = (m.text(a[1]), m.text(a[2]));
        m.note(|| format!("[{caption}] {text}"));
        1 // IDOK
    },
    LoadIconA(2) => |m, a| {
        find_resource(m, resource_type::GROUP_ICON, a[1]).map_or(0, |index| ICON_TAG | index as u32)
    },
    LoadCursorA(2) => |m, a| {
        if a[0] == 0 {
            // A system cursor: the handle carries its identifier.
            return CURSOR_TAG | (a[1] & 0xFFFF);
        }
        find_resource(m, resource_type::GROUP_CURSOR, a[1]).map_or(0, |index| CURSOR_TAG | 0x1_0000 | index as u32)
    },
    LoadAcceleratorsA(2) => |m, a| {
        // The executable has no accelerator tables; the lookup fails as on Windows.
        find_resource(m, resource_type::ACCELERATOR, a[1]).map_or(0, |index| 0x0400_0000 | index as u32)
    },
    TranslateAcceleratorA(3) => |_, a| -> HostResult {
        if a[1] == 0 { Ok(0) } else { not_implemented("TranslateAcceleratorA with a table".into()) }
    },
    RegisterClassA(1) => |m, a| {
        let words: [u32; 10] = m.read_words(a[0]);
        m.register_class(&words)
    },
    RegisterClassExA(1) => |m, a| {
        // WNDCLASSEXA is WNDCLASSA between a size field and a small icon.
        let words: [u32; 12] = m.read_words(a[0]);
        m.register_class(&words[1..11])
    },
    CreateWindowExA(12) => |m, a| m.create_window(a),
    DestroyWindow(1) => |m, a| -> HostResult { Ok(m.destroy_window(a[0], true)? as u32) },
    DefWindowProcA(4) => |m, a| m.default_window_procedure(a),
    ShowWindow(2) => |m, a| -> HostResult { Ok(m.show_window(a[0], a[1] != 0)? as u32) },
    UpdateWindow(1) => |m, a| -> HostResult {
        let pending = m.os.user.window(a[0]).is_some_and(|w| w.visible && w.needs_paint);
        if pending {
            m.send_message(a[0], WM_PAINT, 0, 0)?;
        }
        Ok(1)
    },
    SetFocus(1) => |m, a| -> HostResult {
        let old = m.os.user.active;
        if m.os.user.window(a[0]).is_some() {
            m.activate(a[0])?;
        }
        Ok(old)
    },
    SendMessageA(4) => |m, a| m.send_message(a[0], a[1], a[2], a[3]),
    PostQuitMessage(1) => |m, a| {
        m.os.user.quit = Some(a[0]);
        0
    },
    PeekMessageA(5) => |m, a| {
        const PM_REMOVE: u32 = 1;
        match m.next_message(a[4] & PM_REMOVE != 0) {
            Some(message) => {
                m.write_message(a[0], &message);
                1
            }
            None => 0,
        }
    },
    GetMessageA(4) => |m, a| -> HostResult {
        match m.next_message(true) {
            Some(message) => {
                m.write_message(a[0], &message);
                Ok((message.message != WM_QUIT) as u32)
            }
            // Blocking with nothing that could ever arrive: needs the host event loop.
            None => not_implemented("GetMessageA on an empty queue".into()),
        }
    },
    // Key messages produce character messages once keyboard input exists.
    TranslateMessage(1) => |_, _| 0,
    DispatchMessageA(1) => |m, a| -> HostResult {
        let [window, message, wparam, lparam]: [u32; 4] = m.read_words(a[0]);
        m.send_message(window, message, wparam, lparam)
    },
    BeginPaint(2) => |m, a| {
        // PAINTSTRUCT: device context, erase flag, update rectangle, reserved.
        let Some(window) = m.os.user.window_mut(a[0]) else { return 0 };
        window.needs_paint = false;
        let (width, height) = window.client_size();
        let (width, height) = (width as u32, height as u32);
        let context = a[0] | 1;
        let _ = m.memory.load(a[1], &[0; 64]);
        m.write_words(a[1], &[context, 0, 0, 0, width, height]);
        context
    },
    EndPaint(2) => |_, _| 1,
    ShowCursor(1) => |m, a| {
        m.os.user.cursor_count += if a[0] != 0 { 1 } else { -1 };
        m.os.user.cursor_count as u32
    },
    ClientToScreen(2) => |m, a| {
        let Some((x, y)) = m.os.user.window(a[0]).map(Window::client_origin) else { return 0 };
        let [px, py]: [u32; 2] = m.read_words(a[1]);
        m.write_words(a[1], &[px.wrapping_add(x as u32), py.wrapping_add(y as u32)]) as u32
    },
    GetWindowRect(2) => |m, a| {
        let Some(w) = m.os.user.window(a[0]) else { return 0 };
        let rect = [w.x as u32, w.y as u32, (w.x + w.width) as u32, (w.y + w.height) as u32];
        m.write_words(a[1], &rect) as u32
    },
    GetClientRect(2) => |m, a| {
        let Some(w) = m.os.user.window(a[0]) else { return 0 };
        let (width, height) = w.client_size();
        let rect = [0, 0, width as u32, height as u32];
        m.write_words(a[1], &rect) as u32
    },
    AdjustWindowRect(3) => |m, a| adjust_window_rect(m, a[0], a[1], 0, a[2] != 0),
    AdjustWindowRectEx(4) => |m, a| adjust_window_rect(m, a[0], a[1], a[3], a[2] != 0),
    GetSystemMetrics(1) => |m, a| -> HostResult {
        const SM_CXSCREEN: u32 = 0;
        const SM_CYSCREEN: u32 = 1;
        match a[0] {
            SM_CXSCREEN => Ok(m.os.user.display.width),
            SM_CYSCREEN => Ok(m.os.user.display.height),
            other => not_implemented(format!("GetSystemMetrics({other})")),
        }
    },
    SystemParametersInfoA(4) => |m, a| -> HostResult {
        // The program tells Windows 98 that a screen saver is running, the
        // old way of disabling Alt+Tab and Ctrl+Alt+Del in full screen.
        const SPI_SCREENSAVERRUNNING: u32 = 0x61;
        match a[0] {
            SPI_SCREENSAVERRUNNING => {
                if a[2] != 0 {
                    let _ = m.memory.write(a[2], 4, 0); // previous state: not running
                }
                Ok(1)
            }
            other => not_implemented(format!("SystemParametersInfoA({other:#x})")),
        }
    },
}

/// Grows a client rectangle to the window rectangle that would contain it.
fn adjust_window_rect(m: &mut Machine, rect: u32, style: u32, ex_style: u32, menu: bool) -> u32 {
    let frame = Frame::of(style, ex_style, menu);
    let [left, top, right, bottom]: [u32; 4] = m.read_words(rect);
    let grown = [
        left.wrapping_sub(frame.left as u32),
        top.wrapping_sub(frame.top as u32),
        right.wrapping_add(frame.right as u32),
        bottom.wrapping_add(frame.bottom as u32),
    ];
    m.write_words(rect, &grown) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_follow_the_window_style() {
        const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
        const WS_POPUP: u32 = 0x8000_0000;
        const WS_CAPTION: u32 = 0x00C0_0000;
        const WS_EX_CLIENTEDGE: u32 = 0x0200;
        // A resizable window: 4-pixel sizing frame, 19-pixel caption.
        assert_eq!(
            Frame::of(WS_OVERLAPPEDWINDOW, 0, false),
            Frame {
                left: 4,
                top: 23,
                right: 4,
                bottom: 4
            }
        );
        assert_eq!(Frame::of(WS_OVERLAPPEDWINDOW, 0, true).top, 42);
        // A full-screen pop-up has no frame at all.
        assert_eq!(Frame::of(WS_POPUP, 0, false), Frame::default());
        // A fixed-size captioned window: 3-pixel frame.
        assert_eq!(
            Frame::of(WS_CAPTION, 0, false),
            Frame {
                left: 3,
                top: 22,
                right: 3,
                bottom: 3
            }
        );
        assert_eq!(
            Frame::of(WS_POPUP, WS_EX_CLIENTEDGE, false),
            Frame {
                left: 2,
                top: 2,
                right: 2,
                bottom: 2
            }
        );
    }
}
