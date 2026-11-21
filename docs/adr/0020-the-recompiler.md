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

## First work on speed (2026-11-22)

Measured in the first level, frames 7,541 to 8,042 of the recording
`level.txt`, without the graphics adapter (`andy bench`):

| | frames a second | ns an instruction | in host functions |
|---|---|---|---|
| interpreter | 85 | 14.3 | 20% |
| blocks, as first written | 177 | 5.0 | 42% |
| blocks, after the changes below | 293 | 1.8 | 66% |

What changed, none of it in what is computed:

- **The x87 model worked out, for every multiplication, division and
  square root, the error of the result by a fused multiply-add**, which
  it needs only for the directed rounding modes and which, on a
  processor built for without that instruction, is a slow library
  routine. It is now worked out only in those modes. This sped the
  interpreter up too.
- **Memory**: a read or write inside one page that exists is one look-up
  and a copy; the rest (two pages, a page not yet there, a log kept) is
  a routine apart.
- **The machine's loop** runs block after block without going round
  itself for as long as a block begins where the last ended.
- The small routines of the x87 model are open to inlining, and the
  written crate is built with thin link-time optimisation.

With the graphics card (an RTX 4070 Ti reached through OpenGL on
Direct3D 12 under WSL) the same frames are made at 151 a second: 1.5 ms
of a frame's 6.6 run the game's code, and 5.1 are host functions, almost
all drawing: `Flip` 0.56 ms a frame, 7 µs for each of some 270 draw
calls, the clears. **The drawing path is now what limits the speed**,
not the game's code (`docs/BACKLOG.md`, "one render pass per frame").

Then the drawing path, the same day (frames still the interpreter's,
89 of 89):

- **A draw from a vertex buffer read and prepared the whole buffer**,
  hundreds of vertices, for the few its triangles use: 6.3 µs a call. It
  reads those between the lowest and the highest index used: 0.74 µs.
- **Consecutive batches with the same state are one draw call** on the
  graphics adapter (the same triangles in the same order).

| in the first level | frames a second | ms a frame | game's code | host functions |
|---|---|---|---|---|
| interpreter, no adapter | 85 | 11.7 | 9.4 | 2.3 |
| blocks, no adapter | 482 | 2.1 | 1.1 | 1.0 |
| blocks, graphics card | 345 | 2.9 | 1.2 | 1.7 |

What is left in a frame on the graphics card: `Flip` 0.24 ms, a clear
0.35 ms, each copy 0.3 ms. Every fill and copy is a render pass and a
submission of its own: next is one submission a frame.

**One submission a frame.** Every fill, copy and pass of triangles was
given to the graphics adapter on its own; they are now recorded and given
together when the frame is presented, or sooner when something must come
after them (a surface read back or written from outside). What is written
to a buffer is there before any work of a submission runs, so each piece
of work writes after what the pending ones use. On the graphics card the
first level goes from 345 to 374 frames a second; the time has moved into
the one submission (0.45 ms a frame through OpenGL on Direct3D 12, which
is how WSL reaches the card; `UNVERIFIED` what it is through Vulkan or
Direct3D 12 on Windows itself).

