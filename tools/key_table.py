#!/usr/bin/env python3
"""Writes crates/runtime/src/keys.rs from the DIK_* definitions of dinput.h.

The names and numbers of keyboard keys are those of DirectInput; they are
converted by program so that none is copied by hand.

    python3 tools/key_table.py ref/dx/dinput.h > crates/runtime/src/keys.rs
"""
import re
import sys

keys = {}
for name, value in re.findall(r"^#define\s+DIK_(\w+)\s+(0x[0-9A-Fa-f]+)", open(sys.argv[1]).read(), re.M):
    keys.setdefault(int(value, 16), name)  # the first name of a code is the usual one

print("""//! The keys of a keyboard by the names and scan codes DirectInput gives
//! them. Written by `tools/key_table.py` from `dinput.h`; do not edit.

/// Every key's name (without the `DIK_` prefix) and its code.
pub const KEYS: &[(&str, u8)] = &[""")
for code in sorted(keys):
    print(f'    ("{keys[code]}", 0x{code:02X}),')
print("""];

/// The code of the key with the given name, whatever its case.
pub fn code(name: &str) -> Option<u8> {
    KEYS.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|&(_, code)| code)
}

/// The name of the key with the given code.
pub fn name(code: u8) -> Option<&'static str> {
    KEYS.iter().find(|&&(_, key)| key == code).map(|&(name, _)| name)
}""")
