//! COM objects implemented by the runtime.
//!
//! The program calls a method by loading a function pointer from the
//! object's method table, so both must exist in its memory. An interface
//! pointer handed to the program is the address of two words: the address of
//! the method table (host function stubs, built once per interface) and the
//! index of the object in the runtime's own store. Several interface
//! pointers can name the same object.

use std::collections::HashMap;

use super::Function;
use crate::Machine;

pub const S_OK: u32 = 0;
pub const E_NOINTERFACE: u32 = 0x8000_4002;
pub const E_INVALIDARG: u32 = 0x8007_0057;

pub struct Interface {
    pub name: &'static str,
    /// Methods in method-table order, `IUnknown` first.
    pub methods: &'static [Function],
}

/// A GUID as it lies in memory.
pub type Guid = [u8; 16];

/// Builds a GUID from the usual `{d1-d2-d3-d4[0..2]-d4[2..8]}` notation.
pub const fn guid(d1: u32, d2: u16, d3: u16, d4: [u8; 8]) -> Guid {
    let (a, b, c) = (d1.to_le_bytes(), d2.to_le_bytes(), d3.to_le_bytes());
    [
        a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d4[0], d4[1], d4[2], d4[3], d4[4], d4[5], d4[6],
        d4[7],
    ]
}

pub const IID_IUNKNOWN: Guid = guid(0, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);

/// Method tables already built, by interface.
#[derive(Debug, Default)]
pub struct Com {
    tables: HashMap<&'static str, u32>,
}

impl Machine {
    fn method_table(&mut self, interface: &'static Interface) -> u32 {
        if let Some(&table) = self.os.com.tables.get(interface.name) {
            return table;
        }
        let table = self
            .os
            .allocate(&mut self.memory, 4 * interface.methods.len() as u32);
        for (slot, method) in interface.methods.iter().enumerate() {
            let stub = self.host_function_address(method);
            self.memory
                .write(table + 4 * slot as u32, 4, stub)
                .expect("table was just allocated");
        }
        self.os.com.tables.insert(interface.name, table);
        table
    }

    /// Creates an interface pointer for object number `index`.
    pub(crate) fn interface_pointer(&mut self, interface: &'static Interface, index: u32) -> u32 {
        let table = self.method_table(interface);
        let pointer = self.os.allocate(&mut self.memory, 8);
        self.memory
            .write(pointer, 4, table)
            .expect("pointer was just allocated");
        self.memory
            .write(pointer + 4, 4, index)
            .expect("pointer was just allocated");
        pointer
    }

    /// The object number behind an interface pointer the program passed.
    pub(crate) fn object_index(&mut self, pointer: u32) -> Option<usize> {
        self.memory
            .read(pointer.wrapping_add(4), 4)
            .map(|index| index as usize)
    }

    pub(crate) fn read_guid(&mut self, address: u32) -> Option<Guid> {
        let bytes = self.memory.dump(address, 16)?;
        bytes.try_into().ok()
    }
}

/// Declares an interface as its method table: `Name(argument count,
/// `this` included) => handler`. A method without a handler is a slot that
/// stops the machine, naming itself, if the program ever calls it.
macro_rules! interface {
    ($vis:vis static $ident:ident = $name:literal; $($method:ident($args:literal) $(=> $handler:expr)?,)*) => {
        $vis static $ident: $crate::win32::com::Interface = $crate::win32::com::Interface {
            name: $name,
            methods: &[$(
                $crate::win32::Function {
                    dll: $name,
                    name: concat!($name, "::", stringify!($method)),
                    args: $args,
                    handler: $crate::win32::com::method!($name, $method $(, $handler)?),
                },
            )*],
        };
    };
}

macro_rules! method {
    ($interface:literal, $method:ident, $handler:expr) => {{
        fn handler(m: &mut $crate::Machine, a: &[u32]) -> $crate::win32::HostResult {
            $crate::win32::invoke($handler, m, a)
        }
        handler
    }};
    ($interface:literal, $method:ident) => {{
        fn handler(_: &mut $crate::Machine, _: &[u32]) -> $crate::win32::HostResult {
            Err($crate::Stop::Unimplemented {
                dll: $interface.into(),
                name: stringify!($method).into(),
            })
        }
        handler
    }};
}

pub(crate) use {interface, method};
