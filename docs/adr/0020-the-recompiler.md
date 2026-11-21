# ADR 0020: The recompiler — blocks of Rust beside the interpreter

Status: accepted (2026-11-22)

## Context

ADR 0009 gives each instruction its meaning once, over a backend, and
foresees two backends: the interpreter, which exists and is checked
against an independent emulator, and one that writes Rust. The
interpreter runs the game at about 13 to 15 ns an instruction: enough to
bring the runtime up, not enough for the game as it is to be shipped.

The runtime is not a set of functions the game's code calls and returns
from. It has threads that give way to one another at exact instruction
counts (ADR 0013), a clock that is the count of instructions, host
functions that call back into the game, native replacements of the
game's functions and of stretches inside them (ADR 0004), and recordings
that must replay the same whatever runs the code (ADR 0017).

## Decision

1. **The game's code is compiled block by block, and the machine stays
   the one machine.** A block is a run of instructions always executed
   together: it begins where control can arrive and ends with a transfer
   of control or before the next such place. `andy recompile` writes one
   Rust function for each (`andy_recomp::emit`, `andy_recomp::blocks`)
   into a crate of its own, which the user's machine compiles into a
   program, `andy-game`, that is `andy` with the blocks linked in.
2. **Nothing else changes.** A block works on the interpreter's own
   state (registers, flags, the x87 model, the memory) and returns to the
   machine's loop, which goes on as before: host calls, callbacks,
   replacements, threads. There is no native call for a call of the
   game: its stack is the game's.
3. **The machine runs a block where one begins, and interprets the
   rest**: an instruction no block covers, and the last instructions
   before the running thread's time is up, so that threads change at the
   very instruction they change at in the interpreter. So a run of
   `andy-game` and a run of `andy` execute the same instructions in the
   same order and count them the same.
4. **The state after a block is the interpreter's**, with one exception:
   a flag nothing reads before it is set again is not computed. Which
   those are is worked out over all blocks: what may be read after a
   block is what the blocks it can go on at read first; where it goes on
   at an address taken from a register or the stack, that is any address
   control can arrive at without the code naming it. An address without a
   block is taken to read every flag.
5. **Where control can arrive**: the targets of the code's own jumps and
   calls; the instruction after every transfer; every address of the
   code stored anywhere as a number, which the image lists (they are
   relocated) and which covers the tables of jumps, the pointers to
   functions and the callbacks; the program's entry; and the places the
   runtime's replacements start and resume at.
6. **The blocks belong to one code.** Texts brought from another
   language and some modifications change numbers inside instructions,
   so the crate records the SHA-256 of the code section it was written
   from, and `andy-game` refuses to run another code (or interprets it,
   with `--interpret`).
7. **What every backend must compute alike is written once**
   (`andy_cpu::native`): the arithmetic of the operations, division,
   `cpuid`, `rdtsc`, the x87 unit's loads and stores. The interpreter
   calls it; the written Rust calls it or is the same expression, and
   values known when a block is written are worked out by it.
8. **Checked against the interpreter, block by block**
   (`--check-blocks`): each block is run, taken back (the memory keeps
   what every written byte held), and run again by the interpreter; the
   registers, the x87 unit bit for bit, the flags that may be read
   afterwards and every byte written must be the same, and the machine
   goes on from the interpreter's state.

## Consequences

- The written crate is a translation of the game's code: it stays in
  `out/`, on the user's machine, like every other derived thing (rule 1,
  ADR 0001).
- Measured on the reference build (2026-11-22): 46,201 blocks covering
  257,151 of the 262,490 instructions found (the rest is reached from
  nowhere); 1.27 million lines of Rust in 32 files, 56 MB; written in
  1.4 s and **compiled in 53 s** (28 threads, 6.7 GB at most). The fear
  that a program of this size would take `rustc` hours did not hold.
- Checked: three recordings (into the first level, the pause menu, the
  options), 5.6 thousand million instructions, every block against the
  interpreter: no difference. Unchecked, the run into the level gives the
  same 89 pictures and the same sound file, byte for byte, as the
  interpreter, and the same count of instructions.
- Speed, first measure, with nothing yet done for it: 3 to 6 ns an
  instruction where the interpreter takes 13 to 15. The machine's loop
  is entered for every block, every access to memory looks its page up,
  and the x87 model is called operation by operation: what
  `docs/BACKLOG.md` lists next.
- A change to `crates/cpu`, to the emitter or to the runtime's
  replacements means recompiling the game; a stale program is refused by
  the hash only if the code differs, so `--check-blocks` on the
  recordings is part of testing such a change.
- Windows: the written Rust has nothing of Linux in it. What `andy-game`
  lacks for Windows is the runtime's own (a sound device, see ADR 0019).
