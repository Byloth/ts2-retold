# Code discovery

Results of `andy analyze` on build `pc-1999-10-29` (2026-11-20). Reproduce with
the command in `CLAUDE.md`. Method: `crates/recomp/src/analysis.rs`.

## Result

| Code section, 896,331 bytes | Bytes | Share |
|---|---|---|
| Instructions | 865,451 | 96.55% |
| Jump tables | 6,956 | 0.78% |
| Alignment padding | 14,248 | 1.59% |
| Data referenced by code | 4,511 | 0.50% |
| Unexplained | 5,165 | 0.58% |

- 262,490 instructions, **130 distinct mnemonics** (the translator's whole
  instruction set).
- 2,414 functions: 1,368 called directly, 225 only through stored pointers,
  820 unreferenced ("orphans").
- 144 jump tables, 8,126 direct calls, 1,504 indirect calls.
- **No conflict on any path reached by direct control flow.** All 22 recorded
  conflicts are on speculative paths.
- All 177 imports are referenced from code.

## Findings

- **Dead code is plentiful.** 820 functions have no reference of any kind: no
  call, no stored pointer. The largest is 19.6 KB. They are aligned to 16
  bytes like every compiled function and have normal prologues, so they are
  real functions that the linker kept (the build evidently lacks
  function-level linking). They are translated like any other function; some
  may be console leftovers worth a look. Why they are dead is `UNVERIFIED`.
- **Unexplained bytes (0.58%) are almost all data tables inside the code
  section**, in two blocks at `0x4A7199` (695 bytes) and `0x4A7471` (4,095
  bytes). Their layout (offset, flags, pointer to a GUID) matches DirectInput
  device data formats from the statically linked DirectInput library
  (`UNVERIFIED`). The remaining ~370 bytes are fragments of hand-written C
  runtime routines and 2–3 byte alignment leftovers.
- **Orphans are only adopted when they look compiled**: aligned to 16 bytes or
  directly following code, and free of instructions a Win32 program never
  executes (`iretd`, port I/O, segment loads…). An earlier, looser rule
  adopted ~250 "functions" out of the DirectInput tables; the give-away was
  154 `iretd` instructions in the census.
- **Four indirect jumps are not jump tables:**
  - `0x413E26` `jmp eax`: a tail call through a function pointer, in game code.
  - `0x4D3EC5`, `0x4D3F51`, `0x4D6AED`: C runtime floating-point helpers
    (maths dispatch, Pentium FDIV workaround). The `int 6` conflicts at
    `0x4D6AFD…` belong to the same helper and never execute on a working CPU.
- **The C runtime occupies the top of the section** (roughly `0x4CC000` up).
  Its functions are the only unaligned ones. They are candidates for
  replacement by native implementations instead of translation; the exact
  boundary is `UNVERIFIED`.

## Known limits of the analysis

- Back-to-back jump tables are read as one (successors over-approximated).
- 775 instructions belong to more than one function (shared tails); the
  translator must duplicate or split them.
- A call to a function that never returns is followed as if it returned.
- Function boundaries of address-taken code are heuristic: a stored pointer to
  code that is also reached by fall-through is treated as a label (4 cases).
- Two speculative overlaps (`0x47C570`, `0x49B255`) are not yet explained.

## Controls (reference build; read for ADR 0021)

- `0x414AF0` (no arguments, one `ret` at `0x414E0A`) reads the devices
  once a frame: the keyboard's 256 bytes into `0x529A00`, the first
  joystick's state into `0x529CD8`. It leaves the mask of actions held, 16
  bits, at `0x529B00`, and an analogue pair for walking at `0x731CD8` (x)
  and `0x731CDC` (y, up positive); `[0x830C34]` = 1 says the pair is a
  stick's and its length counts.
- `0x452180` keeps the mask of the frame before (`0x882794`) and this
  frame's (`0x88279C`); while playing, `0x49EBA0` makes the pad word
  `0x52AD88` from them (previous: `0x52F2FE`). There are no "just pressed"
  words: whoever needs one compares the two.
- Actions (from the controls screen's table at `0x500E58`): up 0x10, right
  0x20, down 0x40, left 0x80, camera left 0x100, camera right 0x200, visor
  0x400, target lock 0x800, menu 0x8, cancel 0x1000, spin 0x2000, jump
  0x4000, fire 0x8000.
- The table of bindings: 38 entries of code and mask at `0x529B10`
  (`{-1, 0}` is empty); codes are keyboard scan codes, or 0x400 and up for
  joystick buttons. Saved in slot 99 (`Toy299.sav`), not in `toy2.cfg`;
  edited by the controls screen `0x49C420`, which reads keys raw.
- Raw keys that go round the actions: F3 to F6, the controls screen, the
  start-up selection, skipping a movie.
- `UNVERIFIED`: everything about the joystick was read, not run.

## Camera (reference build; read for ADR 0021)

- The camera in use is 20 bytes at `0x52ADC0`: a place (three 32-bit
  numbers; 32 to one unit of Direct3D, y downwards) and three angles of 16
  bits, 4096 to the turn. `0x4CE050` makes the view and projection matrices
  from it; Direct3D transforms the vertices.
- `0x405860`, once a frame after the character's update, chooses where
  that comes from: the follower's structure at `0x52F3A0` (`0x4045E0`), the
  visor (`0x4038E0`, while `[0x50A13C]` is not 0), or the scripted camera
  at `0x52B7E8` (`0x403640`, while bit 0 of `0x52B816` is set), and blends
  from one to another over 64 ticks.
- The follower's structure: place eased (+0), place wanted (+0x0C; its x
  and z are what the walking directions are taken from, in `0x433F40`),
  heights followed and looked at (+0x18, +0x1C), angles (+0x20), distance
  (+0x26, wanted 1200 in `0x50A128`), the angle sideways (+0x28), the
  elevation (+0x2E, at rest 0x40).
- `0x4045E0`: eases the distance; from `0x4046CE` decides the two angles,
  in a branch for standing still or on a pole and one for moving (bit 0x40
  of `0x52F0CC` is the "active" camera, which turns behind the character,
  and turns the character with the camera's actions); from `0x4053B1` its
  tail places the camera from the angles, eased or exactly (two marks among
  its variables), and turns it to the character.
- Collision: `0x48A4C0` prepares the level's surfaces around a segment,
  `0x48C860` casts a sphere along one and cuts the movement short.
- Sines: 4096 numbers of 16 bits at `0x4FE788`, 16384 for one. The frame's
  time step is at `0x52F2D4`.
- `UNVERIFIED`: the states in which the camera is forced behind the
  character; what the visor's code does was read, not run.
