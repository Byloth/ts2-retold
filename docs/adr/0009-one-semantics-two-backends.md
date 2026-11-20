# ADR 0009 — Instruction semantics are written once, over an abstract backend

Status: Accepted (2026-11-20)

## Context

A recompiler needs the meaning of every instruction twice: to generate code,
and to check that the generated code is right. Two hand-written copies drift.

## Decision

`andy-cpu::sem` defines each instruction as generic code over a `Backend`
trait (values are opaque; choices go through `select`, loops through `repeat`,
control flow through `jump`/`call`/`ret`). Two backends consume it:

- the **interpreter** computes values immediately;
- the **recompiler** (next) records the same operations and emits Rust.

Flags are computed one by one as plain expressions of operands and result, so
that the recompiler can drop every flag nobody reads without special cases.

## Consequences

- The interpreter is the executable specification. It is verified against an
  independent emulator (`docs/SEMANTICS.md`); generated code will be verified
  against the interpreter.
- The interpreter can run the game before the recompiler exists, which is how
  the runtime (the replacement for Windows and DirectX) gets developed.
- Scope is exactly the instructions present in the executable. An instruction
  outside it traps as unsupported instead of being guessed.
- The interpreter is a development tool. The shipped game is recompiled code.
