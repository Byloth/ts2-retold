//! OLE32: the COM library. Objects here are created by the runtime itself,
//! so initialisation has nothing to do, and the classes that can be created
//! are the ones the runtime has.

use super::{amstream, functions};

const E_INVALIDARG: u32 = 0x8007_0057;
const E_POINTER: u32 = 0x8000_4003;
const REGDB_E_CLASSNOTREG: u32 = 0x8004_0154;

functions! { "ole32.dll";
    CoInitialize(1) => |_, _| 0, // S_OK
    CoUninitialize(0) => |_, _| 0,
    CoCreateInstance(5) => |m, a| {
        // Class, outer object, context, interface, result.
        let Some(class) = m.read_guid(a[0]) else { return E_INVALIDARG };
        if m.memory.write(a[4], 4, 0).is_none() {
            return E_POINTER;
        }
        amstream::create_instance(m, &class, a).unwrap_or_else(|| {
            m.note(|| format!("class {} is not registered", super::ddraw::format_guid(&class)));
            REGDB_E_CLASSNOTREG
        })
    },
}
