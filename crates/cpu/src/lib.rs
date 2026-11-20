//! x86 instruction semantics for the subset the game uses.
//!
//! The meaning of every instruction is written exactly once, in [`sem`], as
//! generic code over a [`sem::Backend`]. The interpreter in [`interp`] is the
//! backend that computes values immediately; the recompiler will be the
//! backend that emits code. Both therefore agree by construction, and the
//! interpreter doubles as the executable specification that generated code is
//! tested against.

pub mod fpu;
pub mod interp;
pub mod memory;
pub mod sem;
