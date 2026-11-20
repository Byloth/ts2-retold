# Third-party licenses

Every dependency or tool is listed here **before** it is adopted
(`CLAUDE.md`, rule 4). Regenerate the crate list with `cargo metadata`.

## Rust crates (checked 2026-11-20)

| Crate | License | Used for |
|---|---|---|
| `iced-x86` 1.22 | MIT | x86 decoder (pure Rust, builds for WASM) |
| `sha2` 0.10 | MIT OR Apache-2.0 | identifying the executable build |
| `serde`, `serde_json` | MIT OR Apache-2.0 | analysis reports |
| `fontdue` 0.9 | MIT OR Apache-2.0 OR Zlib | turning glyph outlines into pixels for GDI text |
| `ttf-parser` 0.25 | MIT OR Apache-2.0 | reading the bundled font's metric tables |
| `png` 0.17 | MIT OR Apache-2.0 | `andy boot --surfaces`: writing surfaces as images for inspection (command-line tool only) |

Transitive dependencies: all `MIT`, `MIT OR Apache-2.0` or `Unlicense OR MIT`,
plus `unicode-ident` (`(MIT OR Apache-2.0) AND Unicode-3.0`, build-time only)
and `foldhash` (`Zlib`, through `fontdue`'s `hashbrown`); `png` brings
`flate2`, `miniz_oxide`, `fdeflate`, `crc32fast`, `adler2`, `simd-adler32`
and `bitflags`, all MIT or dual MIT/Apache-2.0 (some also Zlib or 0BSD).
No copyleft and no noncommercial-only license is in the tree.

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

## External projects consulted, not used

Nothing has been copied from these; their licenses are not yet verified and
must be before any code or data is taken from them.

- `RibShark/ToyStory2Fix` — community fix (timing, widescreen, device enumeration).
- `cheeseandcereal/ToyStory2Resources` — speedrun and reverse-engineering notes.
