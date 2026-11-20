//! DINPUT: DirectInput.
//!
//! The machine has a keyboard and a mouse and no joystick. What they are
//! doing is whatever [`Input`] says; the embedder (a window, a recording being
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
const DIDFT_AXIS: u32 = 0x03;
const DIDFT_BUTTON: u32 = 0x0C;
const DIDFT_ANYINSTANCE: u32 = 0x00FF_FF00;
const DIDC_ATTACHED: u32 = 1;
const MOUSE_AXES: usize = 3;
const MOUSE_BUTTONS: usize = 4;

/// What the input devices are doing right now.
#[derive(Debug, Clone)]
pub struct Input {
    /// Keys held down, by keyboard scan code (the `DIK_*` numbering).
    pub keys: [bool; 256],
    /// Mouse movement along x, y and the wheel since the game last read it.
    pub mouse_motion: [i32; MOUSE_AXES],
    pub mouse_buttons: [bool; MOUSE_BUTTONS],
}

impl Default for Input {
    fn default() -> Self {
        Input { keys: [false; 256], mouse_motion: [0; MOUSE_AXES], mouse_buttons: [false; MOUSE_BUTTONS] }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Keyboard,
    Mouse,
}

/// One control of a device, as a data format maps it into the state block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Key(usize),
    MouseAxis(usize),
    MouseButton(usize),
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
    let devices = [
        (GUID_SYS_MOUSE, DIDEVTYPE_MOUSE, &b"Mouse"[..]),
        (GUID_SYS_KEYBOARD, DIDEVTYPE_KEYBOARD, &b"Keyboard"[..]),
    ];
    for (identifier, kind, name) in devices {
        // A type of zero asks for every device. Asking for joysticks finds none.
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
    if wanted != IID_IUNKNOWN && wanted != IID_IDIRECTINPUTDEVICEA {
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
    let (kind, axes, buttons) = match kind {
        Kind::Keyboard => (DIDEVTYPE_KEYBOARD, 0, 256),
        Kind::Mouse => (DIDEVTYPE_MOUSE, MOUSE_AXES as u32, MOUSE_BUTTONS as u32),
    };
    let mut words = [0; 11];
    words[..6].copy_from_slice(&[size, DIDC_ATTACHED, kind, axes, buttons, 0]);
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
    let (mut next_axis, mut next_button) = (0, 0);
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
            _ => None,
        };
        // A control the device does not have is left out; its place in the
        // state block stays zero.
        if let Some(control) = control {
            let width = if matches!(control, Control::MouseAxis(_)) { 4 } else { 1 };
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
    let mut state = vec![0u8; size as usize];
    let input = &mut m.os.input;
    for (offset, control) in controls {
        let at = offset as usize;
        match control {
            Control::Key(code) => state[at] = if input.keys[code] { 0x80 } else { 0 },
            Control::MouseButton(button) => state[at] = if input.mouse_buttons[button] { 0x80 } else { 0 },
            // Reading a relative axis consumes the movement.
            Control::MouseAxis(axis) => state[at..at + 4]
                .copy_from_slice(&std::mem::take(&mut input.mouse_motion[axis]).to_le_bytes()),
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
}
