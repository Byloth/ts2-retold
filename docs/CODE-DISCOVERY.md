# Code discovery

Results of `andy analyze` on build `pc-1999-10-29` (2026-11-20). Reproduce with
the command in `CLAUDE.md`. Method: `crates/recomp/src/analysis.rs`.

## Result

| Code section, 896,331 bytes | Bytes | Share |
|---|---|---|
| Instructions | 860,823 | 96.04% |
| Jump tables | 6,908 | 0.77% |
| Alignment padding | 14,057 | 1.57% |
| Data referenced by code | 11,818 | 1.32% |
| Unexplained | 2,725 | 0.30% |

- 261,545 instructions, **135 distinct mnemonics** (the translator's whole
  instruction set).
- 2,540 functions: 1,364 called directly, 227 only through stored pointers,
  948 unreferenced ("orphans").
- 138 jump tables, 8,072 direct calls, 1,479 indirect calls.
- **No conflict on any path reached by direct control flow.** All 24 recorded
  conflicts are on speculative paths and are explained below.
- All 177 imports are referenced from code.

## Findings

- **Dead code is plentiful.** 948 functions (137 KB, 16% of the code) have no
  reference of any kind: no call, no stored pointer. The largest is 19.6 KB.
  They have normal prologues and follow padding, so they are real functions
  that the linker kept (the build evidently lacks function-level linking).
  They are translated like any other function; some may be console leftovers
  worth a look. Why they are dead is `UNVERIFIED`.
- **Unexplained bytes (0.30%) are data tables inside the code section**, in two
  blocks at `0x4A7199` (695 bytes) and `0x4A7E12` (1,630 bytes). Their layout
  (offset, flags, pointer to a GUID) matches DirectInput device data formats
  from the statically linked DirectInput library (`UNVERIFIED`). The rest are
  2–3 byte alignment fragments and a few C runtime tables.
- **Four indirect jumps are not jump tables:**
  - `0x413E26` `jmp eax`: a tail call through a function pointer, in game code.
  - `0x4D3EC5`, `0x4D3F51`, `0x4D6AED`: C runtime floating-point helpers
    (maths dispatch, Pentium FDIV workaround). The `int 6` conflicts at
    `0x4D6AFD…` belong to the same helper and never execute on a working CPU.
- **The C runtime occupies the top of the section** (roughly `0x4CC000` up).
  These functions are candidates for replacement by native implementations
  instead of translation; the boundary is `UNVERIFIED`.

## Known limits of the analysis

- Back-to-back jump tables are read as one (successors over-approximated).
- 628 instructions belong to more than one function (shared tails); the
  translator must duplicate or split them.
- A call to a function that never returns is followed as if it returned.
- Function boundaries of address-taken code are heuristic: a stored pointer to
  code that is also reached by fall-through is treated as a label (7 cases).
