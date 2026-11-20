# Phase 0 — Reconnaissance

Date: 2026-11-20. Method: static inspection only (PE parsing, `strings`,
`objdump` linear disassembly, hex dumps of data headers). The game was **not
run**. Each finding is tagged **[V]** verified from the files, or
**[U]** `UNVERIFIED` (inference, to be confirmed).

## 0. Verdict

**GO for static recompilation.** No copy protection, no packing, code in the
clear, relocations present, a small and conventional import surface, and a tiny
codebase (~284k instructions). Details and risks below.

## 1. The copy under inspection

| | |
|---|---|
| Source | Scene release (CLASS, Nov 1999) repackaged by the speedrun community with dgVoodoo, DxWrapper and `ToyStory2Fix.asi` **[V]** |
| `toy2.exe` size | 1,290,290 bytes **[V]** |
| SHA-256 | `023eb6a9459443b34d24cf685591bfeb3b95e1acf579405f6d8fa4407ccbdaf0` **[V]** |
| SHA-1 | `175e57449b204504571202e17e1c75d65e44e4ac` **[V]** |
| MD5 | `9a0b9432ac4dd8c06e7e46d4ceaec3cf` **[V]** |
| File mtime / PE timestamp | 1999-10-29 / 1999-10-29 10:40:52 UTC (they agree) **[V]** |

- Whether this executable is byte-identical to the retail disc is **[U]**. Signs
  in favour: the release notes say "Protection: None", the PE timestamp matches
  the file date, and the community fixes are applied by an external `.asi`
  rather than by patching the exe. **To settle: hash `toy2.exe` from an original
  disc image.** This is the project's reference-copy question (ADR 0003).
- Region/language of this build is **[U]**. Text in the exe is English only; a
  `LANGUAGE` setting read by a `ReadIniFile` routine exists **[V]**, so localized
  builds may differ only in data/config **[U]**. A community autosplitter
  distinguishes "PC US" and "PC UK" builds, so at least two executables exist **[U]**.
- The scene release may have dropped or recompressed content (the repack
  "fixes audio"). Music and FMV are present here, but their fidelity to the disc
  is **[U]**.
- As found, the game does not start: `toy2.err` reports
  `Soft Abort - C:\projects\toy2\Win95.cpp Line 217 … not correctly installed`
  **[V]**. The cause was the missing registry setup: after running
  `regsetup.exe` the game starts (reported by the user, 2026-11-20). With the
  bundled wrappers it falls back to a window at 1920×1080×32 and crashes when
  3840×2160 is selected (user report; cause **[U]**).

## 2. Executable

- PE32 i386 GUI, image base `0x400000`, entry point RVA `0xCFC6C` **[V]**.
- Linker version 6.0 → Microsoft Visual C++ 6 **[V]**. CodeView `NB10` record
  points at `C:\projects\toy2\Release\toy2.pdb` (the PDB itself is not shipped) **[V]**.
- Sections **[V]**:

  | Name | RVA | Virtual size | Raw size | Entropy |
  |---|---|---|---|---|
  | `.text` | 0x1000 | 0xDAD4B (896 KB) | 0xDB000 | 6.72 |
  | `.rdata` | 0xDC000 | 0x2400 | 0x3000 | 5.62 |
  | `.data` | 0xDF000 | 0x96FD34 (**9.9 MB**) | 0x2C000 | 5.62 |
  | `.idata` | 0xA4F000 | 0x125C | 0x2000 | 3.74 |
  | `.rsrc` | 0xA51000 | 0x154F8 | 0x16000 | 4.67 |
  | `.reloc` | 0xA67000 | 0x1773A (96 KB) | 0x18000 | 5.73 |

- **`.reloc` is present** **[V]**. Every absolute address in code and data is
  listed, which removes the pointer-vs-constant ambiguity that normally plagues
  static recompilation.
- `.data` is 9.9 MB virtual against 176 KB on disk **[V]**: the game uses large
  static arrays (console heritage) rather than heap structures. Good for
  save-states and state hashing; bad for running two game instances in one
  address space.
- Source file names leaked through assert strings **[V]**: `toy2.cpp`,
  `Win95.cpp`, `direct6.cpp`, `modesel.cpp` (game/platform) and
  `nu3d\{fmv,hobjload,objcore,objload,world}.c` (engine). Many assert strings
  contain the failing C expression verbatim, which names functions and globals
  (`d3dappi.lpD3DDevice`, `D3DAppCreateSurface`, …).

### Disassembly statistics (linear sweep, approximate) **[V]**

- ~284,000 instructions; 8,130 direct calls; ~1,511 indirect calls;
  ~145 jump-table dispatches; 189 undecodable spots (data in `.text`, mostly
  jump tables **[U]**).
- x87: ~12,700 instructions (4.5%). Integer `imul`/`sar`/`idiv`/`cdq` are far
  more common: gameplay maths looks predominantly **fixed-point** **[U]**.
- `fldcw` ×31, `fnstcw` ×10: mostly MSVC `_ftol` and control-word setup **[U]**.
- `fsin` ×14, `fcos` ×11, `fpatan` ×3, `fsqrt` ×76.
- No MMX/SSE seen. One `rdtsc` and two `cpuid`, clustered at `0x4AB7E7–0x4AB8C9`
  (CPU speed detection **[U]**). `in`/`out` hits are data misdecoded as code **[U]**.

## 3. Copy protection

- **None detected** **[V]**: no SafeDisc/SecuROM section names or strings, normal
  entropy, intact imports, entry point inside `.text`.
- A CD/installation check exists: registry key under
  `TravellersTalesToyStory2` with a `cdpath` value, a `validate.tta` marker file,
  and the "not correctly installed" abort **[V]**. Trivial to satisfy in a
  reimplemented OS layer.
- `DeviceIoControl` is referenced 7 times around `0x4A6E26–0x4A70FA` **[V]**;
  purpose is CD-audio control or a drive check **[U]**.

## 4. Imports (complete DLL list) **[V]**

| DLL | Count | Notes |
|---|---|---|
| KERNEL32 | 78 | files, heap, `CreateThread` (2 call sites), events/semaphores/critical sections, `Sleep`, `DeviceIoControl` |
| USER32 | 49 | window, messages, menus, `GetAsyncKeyState` |
| GDI32 | 27 | DIB sections, fonts, `BitBlt`/`StretchBlt` (launcher dialogs, text) |
| DDRAW | 2 | `DirectDrawCreate`, `DirectDrawEnumerateA` |
| DINPUT | 1 | `DirectInputCreateA` |
| DSOUND | 2 | by ordinal (#1 `DirectSoundCreate`, #2 `DirectSoundEnumerateA`) |
| WINMM | 12 | `timeGetTime` + `mmio*` (WAV reading) |
| ADVAPI32 | 3 | registry read only |
| ole32 | 3 | `CoCreateInstance` (DirectShow for FMV) |

No networking, no `QueryPerformanceCounter`, no `GetTickCount` **[V]**.

## 5. Graphics

- **Direct3D 6 immediate mode on top of DirectDraw** **[V]**: `direct6.cpp`,
  a "DX6SDK Samples Path" string, and a `D3DApp*` helper layer derived from the
  DirectX SDK sample framework.
- Calls seen in assert strings **[V]**: `SetRenderState` (16 sites),
  `SetLightState`, `DrawIndexedPrimitive`, `BeginScene`/`EndScene`,
  viewport `Clear`/`SetBackground`, `CreateMaterial`, texture `Load`,
  surface `Lock`/`Unlock`/`BltFast`/`SetColorKey`, palettes, clipper,
  `SetDisplayMode`, `EnumDisplayModes`, `EnumDevices`, `GetAvailableVidMem`.
- The one draw call quoted in full uses **`D3DVT_TLVERTEX`** (pre-transformed,
  pre-lit vertices) **[V]**. Whether *all* world geometry is submitted that way
  is **[U]**, but it is likely: the engine is a PSX port and also ships its own
  **software renderer** ("native software render") **[V]**, which implies
  transform and lighting happen in game code.
- Palettised textures and colour keys are used **[V]**.
- **Consequences:**
  - A faithful backend is small: screen-space triangles, z-buffer, a handful of
    render states, textures, 2D blits. Reuse of a DX9 layer from another project
    is limited to concepts; the API surface is different and much smaller **[U]**.
  - Enhanced rendering (shadows, reflections, water, real lighting) needs
    **world-space** geometry, which a TL-vertex stream no longer has. Those
    features require hooking the engine *above* its transform stage (in `nu3d`),
    not at the Direct3D boundary. This is the main architectural consequence of
    the graphics ambitions and is recorded as an open design point.
  - Native resolution and widescreen are cheap by comparison (the community fix
    already patches projection and draw distance).

## 6. Audio, input, video, timing

- **Sound effects:** DirectSound; 267 `.wav` files, mostly in `data/sfx` **[V]**.
- **Music:** 20 WAV tracks (~8 MB each, 159 MB total) in `audio/`, read via
  `mmio*` and probably streamed on a worker thread **[U]**. Leftover developer
  listings (`tracks.txt`, `convid.btm`) show the original track mapping **[V]**.
- **Input:** DirectInput (v5-era `DirectInputCreateA`) plus `GetAsyncKeyState` **[V]**.
- **FMV:** `rtlibs/*.dll` are **MPEG-1 system streams with a fake extension**
  (22 files, ~210 MB), played through DirectShow multimedia streaming
  (`amstream.dll`) **[V]**. `data/FMV/fmv.bin` is an index still carrying the
  PSX names (`toy2fmv\tt.str`) **[V]**. Browsers cannot decode MPEG-1 natively:
  a WASM decoder is needed, or FMVs are stubbed first.
- **CPU benchmark at startup:** `CalculateClockSpeed` at `0x47EF90` times ten
  runs of a one-million-iteration floating-point loop and derives a CPU clock
  speed in hertz, observed in the game's own log while booting it in the
  interpreter **[V]** (`docs/BOOT.md`). What the engine does with it is **[U]**.
- **Timing:** `timeGetTime` is the only wall-clock source, 11 references **[V]**.
  How it drives the simulation (fixed tick vs. variable delta) is **[U]** and is
  the first thing to reverse in Phase 1.
- **Threads:** two `CreateThread` call sites (`0x4130A2`, `0x4A84D8`) **[V]**;
  music streaming and/or FMV **[U]**. They must not touch simulation state for
  determinism to hold **[U]**.

## 7. Determinism

- **RNG:** one instance of the MSVC `rand()` LCG at `0x4CF54F` **[V]**, plus a
  `rand.dat` file name in the exe **[V]** (a precomputed random table loaded from
  disk **[U]**; the file is absent from this copy **[V]**).
- **Recorded demos:** `data/PAD/PATH01..14.BIN` are small (160–4096 bytes) files
  of 16-bit pairs that look like run-length-encoded pad states
  (`buttons, frame count`), and the exe references `PAD\PATH00.BIN` and a
  `demo mode` string **[V]** for the data, **[U]** for the interpretation. If
  confirmed, the game already replays input recordings frame by frame, meaning
  the simulation is input-deterministic by construction and ships its own
  regression inputs.
- **Floating point:** x87 is a minority of the code; in WASM, `f32`/`f64`
  arithmetic is bit-deterministic across machines (NaN payloads aside), so
  client and replay server agree with each other. Matching the *original*
  bit-for-bit would additionally require emulating x87 precision-control
  behaviour and `fsin`/`fcos` results; whether that matters is **[U]**.
- **Frame-rate dependence:** the community fix has a "FixFramerate" option and
  its author reports rewriting the game's timing code, so the original loop
  misbehaves on fast machines **[V]** (existence of the fix), **[U]** (mechanism).
  Reported "broken" bosses (lift boss moving too fast, slime boss regenerating
  too fast) are plausibly symptoms of this rather than design flaws **[U]**.

## 8. Data files

554 MB total: 159 MB music, ~210 MB FMV, ~180 MB game data **[V]**.

| Pattern | Guess | Evidence |
|---|---|---|
| `data/levelNN/*.ngn` (up to 8 MB) | PC level/scene container produced by a converter; embeds textures as BMP | starts `04 01 00 00`, contains `tex00` + `BM6` headers; `Pconv.cfg` names it `OUTPUT_FILE` **[V]** |
| `Pconv.cfg`, `.bak`, `PCONV.LOG` | config/log of the developers' PSX→PC converter | plain text: `INPUT_FILE level.dat`, `TEXTURE_FILE level.raw` **[V]** |
| `level*.dat`, `level*.raw` | original PSX scene and texture (VRAM) data, inputs to the converter | per `Pconv.cfg` **[V]**; the exe also references `level.dat`/`level.raw` by name, so some may be read at runtime **[U]** |
| `level*.bin` | PSX per-level code overlay | begins with MIPS prologue `addiu sp,-24; sw ra…` **[V]**; unused on PC **[U]** |
| `level.vis`, `TERRAIN.ALL`, `TERR1.ALL` | visibility and collision/terrain data | names only **[U]** |
| `data/chars*/*.all` + `.anm` | character models and animations (66 pairs) | names (`buzz`, `woody`, `rc`, `zurg1`, `slime`…) **[V]**, format **[U]** |
| `data/gfx/*.raw`/`.ngn` | loading/boss/bonus screens | names **[V]** |
| `data/BITS/memory.bin` | PSX memory-card or pointer table leftover | full of `0x800Dxxxx` (PSX RAM) addresses **[V]** |
| `data/PAD/PATH*.BIN` | demo input recordings | see §7 |
| `audio/house.pk` | precomputed waveform peaks from an audio editor | **[U]** |
| `ToyNNN.sav`, `toy2.cfg` | saves and settings, written next to the exe | `Toy2%02d.sav` format string **[V]** |
| `*.kep`, `*.kp2`, `*.new`, `*.bak` | developer leftovers | **[U]** |

Level directories `level00`–`level19` exist; several of the higher ones contain
only a converter config **[V]**.

## 9. Timed races and challenges

- **Partially verified.** Dialogue strings in the exe confirm: a race against
  R.C. around the garage (3 laps), another around the garden (needs the rocket
  boots), a race against a flying saucer along a zipline course, and a race
  against a token down a shaft **[V]**. A `minirace` music track exists **[V]**.
- "Most levels have one" is **[U]**: four distinct races are evidenced so far.
  A full count needs the level scripts.

## 10. Prior art

- No public decompilation or recompilation of the PC version was found **[U]**
  (one web search, not exhaustive).
- `cheeseandcereal/ToyStory2Resources`: reverse-engineering and speedrun
  resources, memory addresses, autosplitter. License not checked.
- `RibShark/ToyStory2Fix`: the `.asi` fix in this copy; a map of known patch
  points (timing, projection, draw distance, device enumeration). License not
  checked.
- The Cutting Room Floor documents debug features of the Windows build,
  including a frame-by-frame mode.

Nothing from these has been used or copied. Licenses must be recorded in
`docs/LICENSES.md` before any use.

## 11. Risks, ranked

1. **Engine globals vs. many players.** 9.9 MB of static state built for one
   player. Ghosts and presence are feasible; shared-state co-op needs real
   understanding of the entity system.
2. **Enhanced graphics need a hook above the transform stage** (§5).
3. **Indirect calls and jump tables** (~1,650 sites) need correct target
   recovery; relocations make this tractable.
4. **Reference copy** is a scene release until a disc image is hashed (§1).
5. **Threads** in a WASM build: the audio/FMV threads should be replaced by
   host-side implementations rather than recompiled.
6. **Legal exposure** grows the moment the repository is public; rules in
   `CLAUDE.md` are the mitigation.

## 12. Toolchain state **[V]**

Present: `rustc 1.100.0`, `cargo`, `git`, `gh`, `objdump`; 28 cores, 31 GB RAM
visible to WSL. Missing: `wasm32-unknown-unknown` target, mingw-w64,
`wasm-pack`/`wasm-bindgen-cli`, any x86 disassembly library.
