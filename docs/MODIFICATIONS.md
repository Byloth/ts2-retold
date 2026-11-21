# Modifications

Everything the port does differently from the original game (ADR 0004). The
source of truth is `crates/runtime/src/mods.rs`; `andy mods` prints the same
list. With every entry off the port behaves like the original.

| Identifier | Class | Affects simulation | On by default | What it does |
|---|---|---|---|---|
| `language-italian` | feature | no | no | The game in Italian (ADR 0016), from the user's own Italian disc (`TS2_ITALIAN_DIR`): the places of the executable that refer to a text refer to the text the Italian executable has in the same place. **Incomplete**: texts only, and only where the code of the two executables is the same; letters with accents and some signs are drawn wrongly (the English font lacks them), and speech, movies and screens are still English. |
| `skip-mode-select` | feature | no | yes | The start-up "Screen Mode Select" window does not exist: the game's routine for it is replaced by one that makes the choice the original proposes when nothing was saved (the first hardware 3D device and the first mode at least 480 lines high: 640×480 at 16 bits, full screen). Choices saved in `toy2.cfg` are ignored, since they index lists that differ from one machine to another. |

## How modifications attach

A modification that changes what the game's code does **replaces a function**
(`crates/runtime/src/game.rs`): when control reaches the function's address,
a native function runs in its place and returns to the caller. The original
is never executed, and a recompiled build leaves it out altogether. Addresses
are those of one exact build of the executable.

`skip-mode-select` replaces the function at `0x432020`. The replacement
writes the same records and variables the original leaves behind, following
the original's own default path; a unit test checks the selection rule.

With the modification off the original routine runs and its window would
have to be shown, which needs GDI text (`docs/BACKLOG.md`); until then the
machine stops at the first text output.

`language-italian` does not replace a function: it changes the image before
it is loaded (`crates/recomp/src/language.rs`). The addresses of the
reference executable are paired with those of the Italian one through
their code (`crates/recomp/src/pairing.rs`); each place that refers to a
text is made to refer to the Italian text of the paired place, kept in a
section added after the image. Nothing of either executable is in the
repository: the modification is computed from the two files when the game
is started.

## Not modifications

Choices about the machine the game runs on are not listed here, because the
original game also met different machines: the display adapter (ADR 0012),
Windows 98 and the install location (ADR 0011), the virtual clock.
