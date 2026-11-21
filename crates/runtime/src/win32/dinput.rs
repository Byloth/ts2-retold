//! DINPUT: DirectInput.
//!
//! The machine has a keyboard, a mouse and, while a joypad is plugged in
//! ([`Input::pad`]), one joystick. What they are doing is whatever
//! [`Input`] says; the embedder (a window, a recording being
//! replayed) writes it and the game reads it through its devices. Reads are
//! the only way input reaches the game here, which makes this the layer
//! where input is recorded and replayed.

use super::com::{E_INVALIDARG, E_NOINTERFACE, Guid, IID_IUNKNOWN, S_OK, guid, interface};
use super::{HostResult, functions};
use crate::Machine;

const DI_OK: u32 = S_OK;
const S_FALSE: u32 = 1;
const DIERR_INVALIDPARAM: u32 = E_INVALIDARG;
/// No device with that identifier.
const DIERR_DEVICENOTREG: u32 = 0x8004_0154;
const DIERR_NOTACQUIRED: u32 = 0x8007_000C;
/// A property or format cannot change while the device is acquired.
const DIERR_ACQUIRED: u32 = 0x8007_00AA;
const DIERR_OLDDIRECTINPUTVERSION: u32 = 0x8007_047E;
const DIERR_BETADIRECTINPUTVERSION: u32 = 0x8007_0481;
const CLASS_E_NOAGGREGATION: u32 = 0x8004_0110;

const IID_IDIRECTINPUTA: Guid =
    guid(0x89521360, 0xAA8A, 0x11CF, [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00]);
const IID_IDIRECTINPUTDEVICEA: Guid =
    guid(0x5944E680, 0xC92E, 0x11CF, [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00]);
/// The device interface of DirectInput 5: the first one's methods, then
/// those of force feedback and `Poll`.
const IID_IDIRECTINPUTDEVICE2A: Guid =
    guid(0x5944E682, 0xC92E, 0x11CF, [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00]);
/// The one joystick's own identifier: the machine's, not a real product's.
const GUID_THE_JOYPAD: Guid =
    guid(0x616E6479, 0x7061, 0x6431, [0x80, 0x00, 0x61, 0x6E, 0x64, 0x79, 0x00, 0x01]);
const GUID_RX_AXIS: Guid = guid(0xA36D02F4, 0xC9F3, 0x11CF, [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00]);
const GUID_RY_AXIS: Guid = guid(0xA36D02F5, 0xC9F3, 0x11CF, [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00]);
const GUID_RZ_AXIS: Guid = guid(0xA36D02E3, 0xC9F3, 0x11CF, [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00]);
const GUID_SYS_MOUSE: Guid =
    guid(0x6F1D2B60, 0xD5A0, 0x11CF, [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00]);
const GUID_SYS_KEYBOARD: Guid =
    guid(0x6F1D2B61, 0xD5A0, 0x11CF, [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00]);
const GUID_X_AXIS: Guid = guid(0xA36D02E0, 0xC9F3, 0x11CF, [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00]);
const GUID_Y_AXIS: Guid = guid(0xA36D02E1, 0xC9F3, 0x11CF, [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00]);
const GUID_Z_AXIS: Guid = guid(0xA36D02E2, 0xC9F3, 0x11CF, [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00]);

/// DirectInput 3 and 5 are the versions a program of the time asks for.
const VERSIONS: std::ops::RangeInclusive<u32> = 0x0300..=0x05FF;
const DIDEVTYPE_MOUSE: u32 = 2 | 2 << 8; // a traditional mouse
const DIDEVTYPE_KEYBOARD: u32 = 3 | 4 << 8; // an enhanced PC keyboard
const DIDEVTYPE_JOYSTICK: u32 = 4 | 4 << 8; // a game pad
const DIDFT_AXIS: u32 = 0x03;
const DIDFT_BUTTON: u32 = 0x0C;
const DIDFT_POV: u32 = 0x10;
/// The device must be polled before its state is read.
const DIDC_POLLEDDEVICE: u32 = 2;
const DIDFT_ANYINSTANCE: u32 = 0x00FF_FF00;
const DIDC_ATTACHED: u32 = 1;
pub const MOUSE_AXES: usize = 3;
pub const MOUSE_BUTTONS: usize = 4;
/// A joypad's axes in DirectInput's order: x, y, z, and the rotations about
/// them. On a pad with two sticks the left one is x and y, the right one
/// the rotations about x and y, and the triggers share z.
pub const PAD_AXES: usize = 6;
pub const PAD_BUTTONS: usize = 16;

/// A joypad plugged in, and what it is doing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pad {
    /// Each axis from -32768 to 32767, 0 at rest; y grows downwards, as
    /// DirectInput has it.
    pub axes: [i16; PAD_AXES],
    pub buttons: [bool; PAD_BUTTONS],
    /// The direction pad: one of eight directions clockwise from up, or
    /// none.
    pub hat: Option<u8>,
}

/// What the input devices are doing right now.
#[derive(Debug, Clone)]
pub struct Input {
    /// Keys held down, by keyboard scan code (the `DIK_*` numbering).
    pub keys: [bool; 256],
    /// Mouse movement along x, y and the wheel that has arrived since the
    /// game last read the keyboard: written by the embedder.
    pub mouse_motion: [i32; MOUSE_AXES],
    /// The movement of the frame the game is making: what had arrived when
    /// it read the keyboard. Input goes by that one clock (ADR 0017).
    pub frame_motion: [i32; MOUSE_AXES],
    pub mouse_buttons: [bool; MOUSE_BUTTONS],
    /// The joypad, while one is plugged in.
    pub pad: Option<Pad>,
}

impl Default for Input {
    fn default() -> Self {
        Input {
            keys: [false; 256],
            mouse_motion: [0; MOUSE_AXES],
            frame_motion: [0; MOUSE_AXES],
            mouse_buttons: [false; MOUSE_BUTTONS],
            pad: None,
        }
    }
}

impl Input {
    /// At a read of the keyboard: what the mouse has moved since the read
    /// before becomes the movement of this frame.
    pub(crate) fn take_frame_motion(&mut self) {
        self.frame_motion = std::mem::take(&mut self.mouse_motion);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Keyboard,
    Mouse,
    Joystick,
}

/// One control of a device, as a data format maps it into the state block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Key(usize),
    MouseAxis(usize),
    MouseButton(usize),
    PadAxis(usize),
    PadButton(usize),
    /// The direction pad, or a place for one the device does not have.
    PadHat(bool),
}

#[derive(Debug)]
struct Device {
    references: u32,
    pointer: u32,
    kind: Kind,
    /// Size of the state block and where each control goes in it, once a
    /// data format is set.
    format: Option<(u32, Vec<(u32, Control)>)>,
    acquired: bool,
}

#[derive(Debug, Default)]
pub struct State {
    /// Reference counts and interface pointers of the DirectInput objects.
    objects: Vec<(u32, u32)>,
    devices: Vec<Device>,
}

impl Machine {
    fn input_object(&mut self, this: u32) -> Option<&mut (u32, u32)> {
        let index = self.object_index(this)?;
        self.os
            .dinput
            .objects
            .get_mut(index)
            .filter(|(references, pointer)| *references > 0 && *pointer == this)
    }

    fn input_device(&mut self, this: u32) -> Option<&mut Device> {
        let index = self.object_index(this)?;
        self.os.dinput.devices.get_mut(index).filter(|device| device.references > 0 && device.pointer == this)
    }
}

functions! { "DINPUT.dll";
    DirectInputCreateA(4) => |m, a| {
        // Instance, version, result, outer object.
        let (version, out, outer) = (a[1], a[2], a[3]);
        if outer != 0 {
            return CLASS_E_NOAGGREGATION;
        }
        if version < *VERSIONS.start() {
            return DIERR_OLDDIRECTINPUTVERSION;
        }
        if version > *VERSIONS.end() {
            return DIERR_BETADIRECTINPUTVERSION;
        }
        let index = m.os.dinput.objects.len() as u32;
        let pointer = m.interface_pointer(&DIRECT_INPUT, index);
        m.os.dinput.objects.push((1, pointer));
        if m.memory.write(out, 4, pointer).is_some() { DI_OK } else { DIERR_INVALIDPARAM }
    },
}

// --- IDirectInputA --------------------------------------------------------

fn object_query_interface(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(wanted) = m.read_guid(a[1]) else { return DIERR_INVALIDPARAM };
    let Some(object) = m.input_object(a[0]) else { return DIERR_INVALIDPARAM };
    if wanted != IID_IUNKNOWN && wanted != IID_IDIRECTINPUTA {
        let _ = m.memory.write(a[2], 4, 0);
        return E_NOINTERFACE;
    }
    object.0 += 1;
    if m.memory.write(a[2], 4, a[0]).is_some() { S_OK } else { DIERR_INVALIDPARAM }
}

fn object_add_ref(m: &mut Machine, a: &[u32]) -> u32 {
    m.input_object(a[0]).map_or(0, |object| {
        object.0 += 1;
        object.0
    })
}

fn object_release(m: &mut Machine, a: &[u32]) -> u32 {
    m.input_object(a[0]).map_or(0, |object| {
        object.0 -= 1;
        object.0
    })
}

fn create_device(m: &mut Machine, a: &[u32]) -> u32 {
    let (identifier, out, outer) = (a[1], a[2], a[3]);
    if outer != 0 {
        return CLASS_E_NOAGGREGATION;
    }
    if m.input_object(a[0]).is_none() {
        return DIERR_INVALIDPARAM;
    }
    let kind = match m.read_guid(identifier) {
        Some(GUID_SYS_KEYBOARD) => Kind::Keyboard,
        Some(GUID_SYS_MOUSE) => Kind::Mouse,
        Some(GUID_THE_JOYPAD) if m.os.input.pad.is_some() => Kind::Joystick,
        Some(_) => return DIERR_DEVICENOTREG,
        None => return DIERR_INVALIDPARAM,
    };
    m.note(|| format!("{kind:?}"));
    let index = m.os.dinput.devices.len() as u32;
    let pointer = m.interface_pointer(&INPUT_DEVICE, index);
    m.os.dinput.devices.push(Device { references: 1, pointer, kind, format: None, acquired: false });
    if m.memory.write(out, 4, pointer).is_some() { DI_OK } else { DIERR_INVALIDPARAM }
}

/// `EnumDevices`: calls back once per attached device of the wanted type.
fn enum_devices(m: &mut Machine, a: &[u32]) -> HostResult {
    let (wanted_type, callback, context) = (a[1], a[2], a[3]);
    if m.input_object(a[0]).is_none() {
        return Ok(DIERR_INVALIDPARAM);
    }
    let mut devices = vec![
        (GUID_SYS_MOUSE, DIDEVTYPE_MOUSE, &b"Mouse"[..]),
        (GUID_SYS_KEYBOARD, DIDEVTYPE_KEYBOARD, &b"Keyboard"[..]),
    ];
    if m.os.input.pad.is_some() {
        devices.push((GUID_THE_JOYPAD, DIDEVTYPE_JOYSTICK, &b"Joypad"[..]));
    }
    for (identifier, kind, name) in devices {
        // A type of zero asks for every device.
        if wanted_type != 0 && wanted_type != kind & 0xFF {
            continue;
        }
        // DIDEVICEINSTANCE (the DirectInput 3 layout, a prefix of the later
        // one): size, instance GUID, product GUID, device type, and two
        // names of 260 characters.
        const SIZE: u32 = 4 + 16 + 16 + 4 + 260 + 260;
        let mut block = vec![0u8; SIZE as usize];
        block[0..4].copy_from_slice(&SIZE.to_le_bytes());
        block[4..20].copy_from_slice(&identifier);
        block[20..36].copy_from_slice(&identifier);
        block[36..40].copy_from_slice(&kind.to_le_bytes());
        block[40..40 + name.len()].copy_from_slice(name);
        block[300..300 + name.len()].copy_from_slice(name);
        if m.with_stack_bytes(&block, |m, at| m.call_guest(callback, &[at, context]))? == 0 {
            break;
        }
    }
    Ok(DI_OK)
}

interface! { static DIRECT_INPUT = "IDirectInputA";
    QueryInterface(3) => object_query_interface,
    AddRef(1) => object_add_ref,
    Release(1) => object_release,
    CreateDevice(4) => create_device,
    EnumDevices(5) => enum_devices,
    GetDeviceStatus(2),
    RunControlPanel(3),
    Initialize(3),
}

// --- IDirectInputDeviceA --------------------------------------------------

fn device_query_interface(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(wanted) = m.read_guid(a[1]) else { return DIERR_INVALIDPARAM };
    let Some(device) = m.input_device(a[0]) else { return DIERR_INVALIDPARAM };
    // The one table of methods serves both device interfaces: the second
    // adds methods after the first's.
    if ![IID_IUNKNOWN, IID_IDIRECTINPUTDEVICEA, IID_IDIRECTINPUTDEVICE2A].contains(&wanted) {
        let _ = m.memory.write(a[2], 4, 0);
        return E_NOINTERFACE;
    }
    device.references += 1;
    if m.memory.write(a[2], 4, a[0]).is_some() { S_OK } else { DIERR_INVALIDPARAM }
}

fn device_add_ref(m: &mut Machine, a: &[u32]) -> u32 {
    m.input_device(a[0]).map_or(0, |device| {
        device.references += 1;
        device.references
    })
}

fn device_release(m: &mut Machine, a: &[u32]) -> u32 {
    m.input_device(a[0]).map_or(0, |device| {
        device.references -= 1;
        device.references
    })
}

fn get_capabilities(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(kind) = m.input_device(a[0]).map(|device| device.kind) else { return DIERR_INVALIDPARAM };
    // DIDEVCAPS: size, flags, type, axes, buttons, point-of-view hats, and
    // five force-feedback words in the DirectInput 5 layout.
    let size = m.memory.read(a[1], 4).unwrap_or(0);
    if size != 24 && size != 44 {
        return DIERR_INVALIDPARAM;
    }
    let (kind, flags, axes, buttons, hats) = match kind {
        Kind::Keyboard => (DIDEVTYPE_KEYBOARD, DIDC_ATTACHED, 0, 256, 0),
        Kind::Mouse => (DIDEVTYPE_MOUSE, DIDC_ATTACHED, MOUSE_AXES as u32, MOUSE_BUTTONS as u32, 0),
        Kind::Joystick => {
            (DIDEVTYPE_JOYSTICK, DIDC_ATTACHED | DIDC_POLLEDDEVICE, PAD_AXES as u32, PAD_BUTTONS as u32, 1)
        }
    };
    let mut words = [0; 11];
    words[..6].copy_from_slice(&[size, flags, kind, axes, buttons, hats]);
    if m.write_words(a[1], &words[..(size / 4) as usize]) { DI_OK } else { DIERR_INVALIDPARAM }
}

/// `SetDataFormat`: the program says where it wants each control's state.
fn set_data_format(m: &mut Machine, a: &[u32]) -> u32 {
    let Some((kind, acquired)) = m.input_device(a[0]).map(|device| (device.kind, device.acquired)) else {
        return DIERR_INVALIDPARAM;
    };
    if acquired {
        return DIERR_ACQUIRED;
    }
    // DIDATAFORMAT: size, object size, flags, data size, object count, objects.
    let [size, object_size, _, data_size, count, objects]: [u32; 6] = m.read_words(a[1]);
    if size != 24 || object_size != 16 || count > 1024 {
        return DIERR_INVALIDPARAM;
    }
    let mut controls = Vec::with_capacity(count as usize);
    let (mut next_axis, mut next_button, mut next_hat) = (0, 0, 0);
    for n in 0..count {
        // DIOBJECTDATAFORMAT: GUID pointer, offset, type, flags.
        let [identifier, offset, kind_and_instance, _]: [u32; 4] = m.read_words(objects + 16 * n);
        let instance = kind_and_instance & DIDFT_ANYINSTANCE;
        let any_instance = instance == DIDFT_ANYINSTANCE;
        let instance = (instance >> 8) as usize;
        let identifier = if identifier == 0 { None } else { m.read_guid(identifier) };
        let control = match kind {
            Kind::Keyboard if kind_and_instance & DIDFT_BUTTON != 0 && !any_instance && instance < 256 => {
                Some(Control::Key(instance))
            }
            Kind::Mouse if kind_and_instance & DIDFT_AXIS != 0 => {
                // An axis is named by its GUID, by number, or just "the next one".
                let axis = match identifier {
                    Some(GUID_X_AXIS) => 0,
                    Some(GUID_Y_AXIS) => 1,
                    Some(GUID_Z_AXIS) => 2,
                    _ if any_instance => next_axis,
                    _ => instance,
                };
                next_axis = axis + 1;
                (axis < MOUSE_AXES).then_some(Control::MouseAxis(axis))
            }
            Kind::Mouse if kind_and_instance & DIDFT_BUTTON != 0 => {
                // Buttons are numbered after the axes.
                let button = if any_instance { next_button } else { instance.wrapping_sub(MOUSE_AXES) };
                next_button = button + 1;
                (button < MOUSE_BUTTONS).then_some(Control::MouseButton(button))
            }
            Kind::Joystick if kind_and_instance & DIDFT_AXIS != 0 => {
                // The joystick's format names its axes; sliders the pad
                // does not have are left out.
                let axis = match identifier {
                    Some(GUID_X_AXIS) => Some(0),
                    Some(GUID_Y_AXIS) => Some(1),
                    Some(GUID_Z_AXIS) => Some(2),
                    Some(GUID_RX_AXIS) => Some(3),
                    Some(GUID_RY_AXIS) => Some(4),
                    Some(GUID_RZ_AXIS) => Some(5),
                    Some(_) => None,
                    None if any_instance => Some(next_axis),
                    None => Some(instance),
                };
                next_axis = axis.map_or(next_axis, |axis| axis + 1);
                axis.filter(|&axis| axis < PAD_AXES).map(Control::PadAxis)
            }
            Kind::Joystick if kind_and_instance & DIDFT_POV != 0 => {
                // The pad has one; the places of others read as centred.
                let hat = if any_instance { next_hat } else { instance };
                next_hat = hat + 1;
                Some(Control::PadHat(hat == 0))
            }
            Kind::Joystick if kind_and_instance & DIDFT_BUTTON != 0 => {
                let button = if any_instance { next_button } else { instance };
                next_button = button + 1;
                (button < PAD_BUTTONS).then_some(Control::PadButton(button))
            }
            _ => None,
        };
        // A control the device does not have is left out; its place in the
        // state block stays zero.
        if let Some(control) = control {
            let wide = matches!(control, Control::MouseAxis(_) | Control::PadAxis(_) | Control::PadHat(_));
            let width = if wide { 4 } else { 1 };
            if offset.checked_add(width).is_none_or(|end| end > data_size) {
                return DIERR_INVALIDPARAM;
            }
            controls.push((offset, control));
        }
    }
    m.input_device(a[0]).expect("checked above").format = Some((data_size, controls));
    DI_OK
}

fn acquire(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(device) = m.input_device(a[0]) else { return DIERR_INVALIDPARAM };
    if device.format.is_none() {
        return DIERR_INVALIDPARAM;
    }
    if std::mem::replace(&mut device.acquired, true) { S_FALSE } else { DI_OK }
}

fn unacquire(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(device) = m.input_device(a[0]) else { return DIERR_INVALIDPARAM };
    if std::mem::replace(&mut device.acquired, false) { DI_OK } else { S_FALSE }
}

/// `GetDeviceState`: the state of every control, laid out as the data format says.
fn get_device_state(m: &mut Machine, a: &[u32]) -> u32 {
    let (size, out) = (a[1], a[2]);
    let Some(device) = m.input_device(a[0]) else { return DIERR_INVALIDPARAM };
    if !device.acquired {
        return DIERR_NOTACQUIRED;
    }
    let (data_size, controls) = device.format.clone().expect("an acquired device has a format");
    if size != data_size {
        return DIERR_INVALIDPARAM;
    }
    // Each read of the keyboard is one tick of recorded input.
    if device.kind == Kind::Keyboard {
        m.os.keyboard_reads += 1;
        if let Some(recording) = &mut m.os.recording {
            recording.apply(m.os.keyboard_reads, &mut m.os.input);
        }
        m.os.input.take_frame_motion();
        if let Some(recorder) = &mut m.os.recorder {
            recorder.observe(m.os.keyboard_reads, &m.os.input);
        }
    }
    let mut state = vec![0u8; size as usize];
    let input = &mut m.os.input;
    for (offset, control) in controls {
        let at = offset as usize;
        match control {
            Control::Key(code) => state[at] = if input.keys[code] { 0x80 } else { 0 },
            Control::MouseButton(button) => state[at] = if input.mouse_buttons[button] { 0x80 } else { 0 },
            // Reading a relative axis consumes the frame's movement.
            Control::MouseAxis(axis) => state[at..at + 4]
                .copy_from_slice(&std::mem::take(&mut input.frame_motion[axis]).to_le_bytes()),
            // A pad pulled out reads as one at rest. An axis goes from 0 to
            // 65535, DirectInput's range for one that was given none.
            Control::PadAxis(axis) => {
                let value = input.pad.map_or(0, |pad| pad.axes[axis]) as i32 + 0x8000;
                state[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
            Control::PadButton(button) => {
                state[at] = if input.pad.is_some_and(|pad| pad.buttons[button]) { 0x80 } else { 0 };
            }
            // A direction in hundredths of a degree clockwise from up, or
            // all ones when centred.
            Control::PadHat(present) => {
                let direction = input.pad.and_then(|pad| pad.hat).filter(|_| present);
                let value = direction.map_or(u32::MAX, |direction| direction as u32 * 4500);
                state[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
    }
    if m.memory.load(out, &state).is_some() { DI_OK } else { DIERR_INVALIDPARAM }
}

interface! { static INPUT_DEVICE = "IDirectInputDeviceA";
    QueryInterface(3) => device_query_interface,
    AddRef(1) => device_add_ref,
    Release(1) => device_release,
    GetCapabilities(2) => get_capabilities,
    EnumObjects(4),
    GetProperty(3),
    SetProperty(3),
    Acquire(1) => acquire,
    Unacquire(1) => unacquire,
    GetDeviceState(3) => get_device_state,
    GetDeviceData(5),
    SetDataFormat(2) => set_data_format,
    SetEventNotification(2),
    // Every device is always available to the one program there is.
    SetCooperativeLevel(3) => |m, a| if m.input_device(a[0]).is_some() { DI_OK } else { DIERR_INVALIDPARAM },
    GetObjectInfo(4),
    GetDeviceInfo(2),
    RunControlPanel(3),
    Initialize(4),
    // IDirectInputDevice2A from here.
    CreateEffect(5),
    EnumEffects(4),
    GetEffectInfo(3),
    GetForceFeedbackState(2),
    SendForceFeedbackCommand(2),
    EnumCreatedEffectObjects(4),
    Escape(2),
    // The joystick is read when polled; the other devices have nothing
    // to poll.
    Poll(1) => |m, a| match m.input_device(a[0]) {
        Some(device) if !device.acquired => DIERR_NOTACQUIRED,
        Some(device) if device.kind == Kind::Joystick => DI_OK,
        Some(_) => S_FALSE,
        None => DIERR_INVALIDPARAM,
    },
    SendDeviceData(5),
}
