//! Static recompiler for the game executable.
//!
//! The library does no I/O: callers hand it the bytes of the user's own
//! executable. Nothing derived from that executable may be committed to the
//! repository (see `docs/adr/0001-local-recompilation-no-derived-code.md`).

pub mod analysis;
pub mod builds;
pub mod difftest;
pub mod language;
pub mod pairing;
pub mod pe;
