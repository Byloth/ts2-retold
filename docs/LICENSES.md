# Third-party licenses

Every dependency or tool is listed here **before** it is adopted
(`CLAUDE.md`, rule 4). Regenerate the crate list with `cargo metadata`.

## Rust crates (checked 2026-11-20)

| Crate | License | Used for |
|---|---|---|
| `iced-x86` 1.22 | MIT | x86 decoder (pure Rust, builds for WASM) |
| `sha2` 0.10 | MIT OR Apache-2.0 | identifying the executable build |
| `serde`, `serde_json` | MIT OR Apache-2.0 | analysis reports |

Transitive dependencies: all `MIT`, `MIT OR Apache-2.0` or `Unlicense OR MIT`,
plus `unicode-ident` (`(MIT OR Apache-2.0) AND Unicode-3.0`, build-time only).
No copyleft and no noncommercial-only license is in the tree.

## External projects consulted, not used

Nothing has been copied from these; their licenses are not yet verified and
must be before any code or data is taken from them.

- `RibShark/ToyStory2Fix` — community fix (timing, widescreen, device enumeration).
- `cheeseandcereal/ToyStory2Resources` — speedrun and reverse-engineering notes.
