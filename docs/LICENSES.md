# Third-party licenses

Every dependency or tool is listed here **before** it is adopted
(`CLAUDE.md`, rule 4). Regenerate the crate list with `cargo metadata`.

## Rust crates (checked 2026-11-20)

| Crate | License | Used for |
|---|---|---|
| `iced-x86` 1.22 | MIT | x86 decoder (pure Rust, builds for WASM) |
| `sha2` 0.10 | MIT OR Apache-2.0 | identifying the executable build |
| `serde`, `serde_json` | MIT OR Apache-2.0 | analysis reports |
| `bytemuck` 1 | Zlib OR Apache-2.0 OR MIT | vertices as plain bytes for the video backend |
| `fontdue` 0.9 | MIT OR Apache-2.0 OR Zlib | turning glyph outlines into pixels for GDI text and for the title in the window's frame |
| `ttf-parser` 0.25 | MIT OR Apache-2.0 | reading the bundled font's metric tables |
| `wgpu` 30 | MIT OR Apache-2.0 | the GPU video backend (`crates/gpu`): Vulkan, Direct3D 12, Metal and WebGPU from one code base |
| `winit` 0.30 | Apache-2.0 | the window of `andy play` (command-line tool only), without its own window frame for Wayland; its tree of platform bindings is MIT, Apache-2.0, Zlib, BSD or ISC |
| `pollster` 1 | MIT OR Apache-2.0 | waiting for the graphics device to open, on native targets, and for the sound server's answers |
| `pulseaudio` 0.3 | MIT | the sound device of `andy play` (command-line tool only): the PulseAudio server's protocol in Rust, no system library; its tree (`mio`, `futures`, `bitflags`, `byteorder`, `enum-primitive-derive`, `num-traits`, `thiserror`, `log` and theirs) is MIT, MIT OR Apache-2.0 or Unlicense OR MIT |
| `cpal` 0.15 | Apache-2.0 | the sound device of `andy play` where there is no PulseAudio server (Windows: WASAPI); a dependency of those targets only, not built on Linux. Its tree there (`windows`, `windows-core`, `dasp_sample` and theirs) is MIT OR Apache-2.0 |
| `png` 0.17 | MIT OR Apache-2.0 | `andy boot --surfaces`: writing surfaces as images for inspection (command-line tool only) |

Transitive dependencies: all `MIT`, `MIT OR Apache-2.0` or `Unlicense OR MIT`,
plus `unicode-ident` (`(MIT OR Apache-2.0) AND Unicode-3.0`, build-time only)
and `foldhash` (`Zlib`, through `fontdue`'s `hashbrown`); `png` brings
`flate2`, `miniz_oxide`, `fdeflate`, `crc32fast`, `adler2`, `simd-adler32`
and `bitflags`, all MIT or dual MIT/Apache-2.0 (some also Zlib or 0BSD).
`wgpu` brings about 130 crates (`naga`, `wgpu-core`, `wgpu-hal`, platform
bindings): MIT, Apache-2.0, Zlib, BSD-2-Clause, ISC or Unlicense, alone or
in combination (checked with `cargo metadata` on 2026-11-20).
No copyleft and no noncommercial-only license is in the tree.

## Code ported into the repository

| Code | License | Used for |
|---|---|---|
| pl_mpeg (Dominic Szablewski, <https://github.com/phoboslab/pl_mpeg>) | MIT; notice in `crates/mpeg/LICENSE-pl_mpeg` | `crates/mpeg` is a port of it to Rust: MPEG-1 demultiplexer, video decoder and Layer II audio decoder (the last with the requantisation corrected, ADR 0015). Its tables are converted by `tools/mpeg_tables.py` and `tools/mpeg_audio_tables.py`. |
| kjmp2 (Martin J. Fiedler, <http://keyj.emphy.de/kjmp2/>) | zlib license (read from the header of `kjmp2.c` 1.1, 2026-11-21); notice in `crates/mpeg/LICENSE-kjmp2` | pl_mpeg's audio decoder is based on it, so the port is an altered version of it too, and says so. |

## Bundled data

| File | License | Used for |
|---|---|---|
| `crates/runtime/assets/fonts/LiberationSans-Regular.ttf` (Liberation Fonts 2.1.5, unmodified) | SIL Open Font License 1.1; license and authors are beside the file | The typeface GDI text is drawn with, standing in for the Windows fonts that cannot be shipped. Chosen because its widths and line metrics are those of Arial. The OFL allows bundling and redistribution with any software provided the license and copyright notice travel with the font; the font is not modified, so the reserved font names are not at issue. It must not be sold by itself. |

## Development tools (not linked, not distributed)

| Tool | License | Used for |
|---|---|---|
| Unicorn 2.1.4 (Python package, QEMU CPU core) | GPL-2.0 | Test oracle for `tools/difftest.py`. Runs as a separate process in a local virtualenv; no code of it is copied or linked into the project. |

## Reference material (read as specification, nothing copied)

| Material | License | Used for |
|---|---|---|
| mingw-w64 / Wine DirectX headers (`ddraw.h`, `d3d.h`, `d3dtypes.h`, `d3dcaps.h`, `dinput.h`, `dsound.h`) | LGPL-2.1 | Structure layouts, method-table order and constant values of the DirectX 6 interfaces. Kept in the git-ignored `ref/` directory. The runtime's definitions are written from these facts, not transcribed. |

| Wine `wingdi.h` (9.0) | LGPL-2.1 | Layout of `TEXTMETRICA` and GDI constant values. Kept in the git-ignored `ref/win`. Facts only, nothing transcribed. |
| Wine `dlls/ddraw/device.c` (9.0) | LGPL-2.1 | How the DirectX 5 render states act on texture stage 0 (ADR 0014), a behaviour Wine established by tests on Windows. Kept in the git-ignored `ref/wine`. The rules are re-expressed, no code is copied. |
| Wine `include/amstream.idl` (9.0) | LGPL-2.1 | The values of the `AMMSF_*` flags. Kept in the git-ignored `ref/wine`. Facts only, nothing transcribed. |
| WSLg's PulseAudio `src/modules/rdp/module-rdp-sink.c` (microsoft/pulseaudio-mirror, branch `working`) | LGPL-2.1 | Read to understand the stall of WSLg's sound (`docs/BACKLOG.md`). Kept in the git-ignored `ref/wslg`. Nothing is taken from it. |

## External projects consulted, not used

Nothing has been copied from these; their licenses are not yet verified and
must be before any code or data is taken from them.

- `RibShark/ToyStory2Fix` — community fix (timing, widescreen, device enumeration).
- `cheeseandcereal/ToyStory2Resources` — speedrun and reverse-engineering notes.
