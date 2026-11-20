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
