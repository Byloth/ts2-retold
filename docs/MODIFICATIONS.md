# Modifications

Everything the port does differently from the original game (ADR 0004). The
source of truth is `crates/runtime/src/mods.rs`; `andy mods` prints the same
list. With every entry off the port behaves like the original.

| Identifier | Class | Affects simulation | On by default | What it does |
|---|---|---|---|---|
| `language` | feature | no | no | The game in another language (ADR 0016), from the user's own disc of that language (`TS2_LANGUAGE_DIR`): Italian, French or Spanish, told by the disc. A language is data and nothing else: the places of the executable that refer to a text refer to the text that language's executable has in the same place; speech, movies and textures are read from that disc. Code, level data and timings stay the reference's. **Incomplete**: only the large typeface of the menus is drawn the one way (`one-typeface`); in the other typefaces letters with accents are still wrong. |
| `one-typeface` | feature | no | yes | Text in the large and the small typeface is drawn by one function with one table from character to glyph, for every language, from one atlas put together from all the discs at hand, with every sign any of them has, each once (ADR 0016): the hyphen, the accents, `¿ ¡ Æ Œ Ç` are there for every language. It also draws the digits and signs the original's function leaves out though the typeface has them (the stroke among them), lays out texts with line breaks, and sets a line closer when it is too wide. English texts come out as in the original, pixel for pixel. |
| `menu-load-save-wording` | feature | no | yes | The main menu's entry for loading and saving reads "load/save" ("carica/salva", "charger/sauvegarder", "cargar/guardar"), where the original has "load game" and the other languages "carica-salva opzioni", "charger-sauvegarder", "cargar-guardar". Needs `one-typeface` for the stroke. Decided by the owner. |
| `menu-arrows-by-text` | fix | no | yes | What a menu draws beside the entry chosen follows that entry's text, worked out when it is drawn. The two arrows of the main menu stand beside the text wherever it begins and ends, and go from one width to the other as they move; the original draws them at two fixed places that suit its first entry, ten letters long: beside a shorter entry they stand far off, and on a longer one (in other languages) they fall on the text. The two pointers of the options menu stand where the original has them, and further apart where an entry is wider than the original's widest, twenty letters. |
| `menu-load-before-options` | feature | no | yes | In the main menu the entry for loading and saving comes before the options (the original has the options second and loading third), in every language. Decided by the owner. |
| `skip-mode-select` | feature | no | yes | The start-up "Screen Mode Select" window does not exist: the game's routine for it is replaced by one that makes the choice the original proposes when nothing was saved (the first hardware 3D device and the first mode at least 480 lines high: 640×480 at 16 bits, full screen). Choices saved in `toy2.cfg` are ignored, since they index lists that differ from one machine to another. |

## How modifications attach

A modification that changes what the game's code does **replaces a function**
(`crates/runtime/src/game.rs`): when control reaches the function's address,
a native function runs in its place and returns to the caller. The original
is never executed, and a recompiled build leaves it out altogether. Addresses
are those of one exact build of the executable.

A replacement can also stand for **a stretch of code inside a function**:
it runs when control reaches the stretch's first instruction, and the game
goes on after the stretch. `menu-arrows-by-text` replaces the stretch of
the main menu (`0x437E61` to `0x437EAE`) that draws the two arrows: it
measures the text of the entry they are at (the large typeface sets its
letters 12 apart) and calls the game's own sprite function with the two
places worked out, where the original passes 64 and 240. The same
modification replaces the stretch of the options menu (`0x4375FB` to
`0x437620`) that draws its two pointers at 48 and 240.

A modification can also be **a change of the image** before it is loaded:
`menu-load-before-options` exchanges, in the main menu's code, the texts
of its second and third entry and the numbers it answers for them
(`crates/recomp/src/menu.rs`).

`skip-mode-select` replaces the function at `0x432020`. The replacement
writes the same records and variables the original leaves behind, following
the original's own default path; a unit test checks the selection rule.

With the modification off the original routine runs and its window would
have to be shown, which needs GDI text (`docs/BACKLOG.md`); until then the
machine stops at the first text output.

`language` does not replace functions: it changes the image before it is
loaded (`crates/recomp/src/language.rs`) and says which files are read
from the other language's disc (`crates/runtime/src/files.rs`). The
addresses of the reference executable are paired with those of the other
build through their code (`crates/recomp/src/pairing.rs`), and each place
that refers to a text is made to refer to the text of the paired place.
Texts referred to from places that could not be aligned, because the code
around them differs, are told apart by their order between two aligned
places (`Hybrid::texts_between`): one for one, two of the other build as
the two lines of one text where it draws in two lines what the reference
draws in one, and a text right after an aligned one as a further line of
it. What cannot be told apart stays in English and is listed every time
the game is started.
Nothing else: no function and no table of the other build is taken, and
the code that runs, with its timings, is the reference's for every
language (ADR 0016, "A language is data").

`one-typeface` has three parts. When the game is started, the one atlas
of each typeface is put together from the discs at hand
(`crates/cli/src/atlas.rs`, `crates/recomp/src/typeface.rs`): the common
cells from the reference disc, then every other sign once, in a fixed
place, from the first disc that has it. What each disc's page holds is
written in the repository, page by page, known by the hash of its pixels.
The atlas takes the place of each disc's page as texture files are read
(`Files::texture_filter`), and the image gets a sheet record with all the
atlas's cells. Then the reference's text functions (`0x49B580` for the
large typeface, `0x49B2A0` and `0x49B3B0` for the small one) are replaced
by native ones (`crates/runtime/src/typeface.rs`) that lay a text out one
way and draw through one table from character to glyph, the same for
every language: with the English texts the pictures are the original's,
pixel for pixel.

`menu-load-save-wording` changes the image: the entry's text is the
project's own, in a section of its own (`crates/recomp/src/menu.rs`).

A language's game can itself be run as a comparison: `andy boot` runs the
Italian executable as it is (the addresses the runtime knows are translated
through the pairing), and the pictures of the two runs with the same input
are compared. Nothing of either executable is in the
repository: the image is computed from the two files when the game is
started.

## Not modifications

Choices about the machine the game runs on are not listed here, because the
original game also met different machines: the display adapter (ADR 0012),
Windows 98 and the install location (ADR 0011), the virtual clock.
