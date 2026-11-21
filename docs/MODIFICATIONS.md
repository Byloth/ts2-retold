# Modifications

Everything the port does differently from the original game (ADR 0004). The
source of truth is `crates/runtime/src/mods.rs`; `andy mods` prints the same
list. With every entry off the port behaves like the original.

| Identifier | Class | Affects simulation | On by default | What it does |
|---|---|---|---|---|
| `language` | feature | no | no | The game in another language (ADR 0016), from the user's own disc of that language (`TS2_LANGUAGE_DIR`): Italian, French or Spanish, told by the disc. A language is data: the places of the executable that refer to a text refer to the text that language's executable has in the same place, with a short list of corrections the project words itself; speech, movies and textures are read from that disc. Level data and code stay the reference's. **Incomplete**: the reference's code cannot draw letters with accents. Until one way of drawing text serves every language, Italian alone borrows 19 functions and four tables of the Italian executable (to be removed), and French and Spanish show accents, and some plain letters, as wrong glyphs. |
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

`language` does not replace functions with native ones: it changes
the image before it is loaded (`crates/recomp/src/language.rs`) and says
which files are read from the Italian disc (`crates/runtime/src/files.rs`).
The addresses of the reference executable are paired with those of the
Italian one through their code (`crates/recomp/src/pairing.rs`). Each
place that refers to a text is made to refer to the Italian text of the
paired place. For Italian alone, and only until text is drawn one way
for every language (ADR 0016), each function listed as language
(`ITALIAN_FUNCTIONS`) is copied from the Italian executable into a section after the image, every
address in it translated through the pairing and every call led to the
reference's function (or to another copied one); the reference's function
begins with a jump to it. Tables of the Italian build that go with its
language (`ITALIAN_TABLES`) are copied the same way, and what referred to
the reference's refers to them. The functions that differ for other reasons
(the Italian build is a later revision) are listed too
(`ITALIAN_COUNTERPARTS`) and stay the reference's.

The hybrid is checked against the Italian game itself: `andy boot` runs the
Italian executable as it is (the addresses the runtime knows are translated
through the pairing), and the pictures of the two runs with the same input
are compared. Nothing of either executable is in the
repository: the image is computed from the two files when the game is
started.

## Not modifications

Choices about the machine the game runs on are not listed here, because the
original game also met different machines: the display adapter (ADR 0012),
Windows 98 and the install location (ADR 0011), the virtual clock.
