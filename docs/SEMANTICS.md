# Instruction semantics and their verification

Code: `crates/cpu` (`sem.rs` semantics, `interp.rs` interpreter, `fpu.rs` x87
model). Decisions: ADR 0009, ADR 0010.

## How it is tested

1. `andy difftest` takes **every distinct instruction encoding found in the
   executable** (44,739 of them, from 262,490 instructions), runs each through
   the interpreter from several random machine states, and writes the case and
   the resulting state as one JSON line. States are biased towards edge values;
   memory nobody wrote is a deterministic pattern, so any address is readable.
2. `tools/difftest.py` replays each case on Unicorn (QEMU's CPU core) and
   compares registers, `eip`, flags (minus those the manuals leave undefined),
   every byte written, and the x87 stack, control and status words.

```sh
python3 -m venv .venv && .venv/bin/pip install unicorn==2.1.4   # once
cargo run --release --bin andy -- difftest --out out/difftest.jsonl --rounds 40
.venv/bin/python tools/difftest.py out/difftest.jsonl --jobs 24
```

Coverage is exactly what the game contains, operand forms included: nothing
is tested that is not needed, and nothing needed goes untested.

## Result (2026-11-20, 40 rounds)

| | |
|---|---|
| Cases | 1,789,160 |
| Mnemonics exercised | 122 |
| Agreeing with the oracle | **1,789,160** |
| Differing | **0** |
| Compared with a tolerance | 640 (transcendental functions, constants) |
| Faulting identically on both sides | 312 (division errors) |

x87 cases run under six control words: double and single precision, each with
round-to-nearest and truncation, plus round-down and round-up.

## Not covered by the oracle

- `fprem`, `fprem1` (80 cases): the oracle's implementation is inaccurate
  (remainder computed as `st0 - st1 * q` in double arithmetic, wrong result
  for `fprem1` when `|st0| < |st1|`). Covered instead by a unit test against
  exact integer arithmetic (40,000 checks).
- `cpuid`, `rdtsc`: results are not a function of the machine state.
- `fnsave`, `frstor`, `fnstenv`, `fldenv`: not implemented (ADR 0010).

## What the comparison found

Worth recording, because each was a real mistake caught mechanically:

- `fprem` quotient bits were assigned to the wrong condition codes (the
  manual's order is C0 = Q2, C3 = Q1, C1 = Q0).
- Arithmetic under a directed rounding mode was rounded to nearest; this led
  to implementing all four modes exactly rather than documenting a gap.
- An exact zero from `x - x` must be negative when rounding down.
- `fscale` turned zero into NaN for huge exponents.
- `push ax` / `pop ax` (16-bit, two sites) were rejected.

And three places where the oracle, not the model, was wrong or unhelpful: it
does not always report a division fault, does not round transcendental results
to single precision, and loses digits in `f2xm1`.
