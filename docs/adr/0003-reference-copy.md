# ADR 0003 — Reference copy of the game

Status: Accepted; reference copy named by the owner (2026-11-20)

## Context

The copy available at the start is a 1999 scene release repackaged with
third-party fixes. The project wants the cleanest possible baseline, with all
changes layered on top as its own catalogued modifications (ADR 0004).

## Decision

- The baseline is the **unmodified retail executable and data**, identified by
  hash. No third-party fix (`.asi`, wrapper DLLs) is part of the baseline.
- The recompiler refuses, or loudly warns about, an executable whose hash is not
  in its list of supported builds.
- Until an original disc image is obtained, work proceeds on the current
  executable (SHA-256 `023eb6a9…bdaf0`), recorded as *provisional*.

## Open action

Obtain an original disc image, hash `toy2.exe` and the data files, and compare.
If they differ, switch the baseline and re-run recon on the differences. Also
record which regional/language builds exist.

## 2026-11-20: compared with the installation disc's files

The owner obtained an archive holding the files of the installation disc
(`Setup.exe`, `Setup\Toy2\HD` with the executable and `data`,
`Setup\Toy2\CD` with `rtlibs`, `audio` and `validate.tta`; kept in the
git-ignored `ref/disc`).

- `Setup\Toy2\HD\toy2.exe` has the same SHA-256, `023eb6a9…bdaf0`.
- All 728 game files of the archive (679 under `HD`, 49 under `CD`) are
  identical to the installed copy's, by size and CRC-32.
- The installed copy has 27 files more: the third-party wrappers and fix
  (`ddraw.dll`, `d3dimm.dll`, `d3d8.dll`, `dsound.dll`, dgVoodoo,
  `scripts\toystory2fix.asi`), saves and settings, release notes, a copy of
  `validate.tta` under `data`, and `audio\music21.wav`, which is not on the
  disc.
- The disc's split confirms the registry values of ADR 0011: `path` is the
  hard-disk part (`data\`), `cdpath` the part left on the CD (`rtlibs`,
  `audio`, `validate.tta`).

The executable and data of the working copy are therefore the disc's own.

**Decision of the owner (2026-11-20): this archive is the reference copy of
the game.** The build `pc-1999-10-29` is no longer provisional. A verified
dump of a pressed disc would still be welcome as a second witness.

### What the reference copy contains

- **One language: English.** The game's texts are in the executable, in
  English only; no other language's text or language name is in it or in
  `data`. Each of the 22 movies has one video and one audio stream. The
  music (23 `.wav` files, 8-bit stereo at 22 kHz, named in the executable)
  and the 61 sound effects have no language.
- **The installer is the multilingual part**: "The Disney Installer" carries
  strings for 13 languages (`Setup\STRINGS\*.lag`) and 14 licence texts,
  and is configured for English with the language choice switched off.
- **A hook for other languages exists**: `toy2.ini` accepts `LANGUAGE` and
  `DEFTEXT` lines. The localised releases did not use it: see below.
- **Leftovers of development** are on the disc: PlayStation executables
  (`data\SLES_020.67`, `SLUS_008.93`, `psx.exe`, `system.cnf`), `.bak` and
  `.btm` build scripts.
- **`audio\music21.wav` is not part of the game.** The installed copy has
  it; the disc does not. It is 0.2 seconds of silence (4484 zero bytes,
  11 kHz mono), dated 11 November 1999, the days of the files the 1999
  repackaging added (`regsetup.exe`, `DeIsL1.isu`). The executable names
  its music files and this name is not among them, nor anywhere else in it.
  It is taken to be an artefact of that repackaging.

The tools run on `ref/game`, which `tools/game-dir-from-disc.sh` makes from
the disc's files by linking the hard-disk part and the CD part side by side,
as a full installation does. The game boots from it exactly as from the
installed copy (same frames, same stopping point), without that copy's
saves and settings.

## 2026-11-20: the other language releases

The owner supplied eight more disc images. Read in place, nothing kept.
Each is one language, with **its own executable** (the texts are compiled
in), so localisation was not done through `toy2.ini`:

| Language | `toy2.exe` bytes | Linked (UTC) | SHA-256 |
|---|---|---|---|
| English (reference copy) | 1290290 | 1999-10-29 10:40 | `023eb6a9459443b34d24cf685591bfeb3b95e1acf579405f6d8fa4407ccbdaf0` |
| French | 1298482 | 2000-01-24 13:43 | `72f9647e601cb39fb450bac7a00375302d63e0ba51fbf0794551b806b3d11ced` |
| English (2003 disc) | 1228800 | 1999-10-29 10:40 | `74bada2d0032379ab506949e839b15773b33bb9377e99af198378690d8b722c9` |
| Norwegian | 1294386 | 2000-02-09 10:55 | `66e853a746b375f2ac38313f86acd2c3ad09cac1677662fe4fe9c55d853788ab` |
| Italian | 1298482 | 2000-02-16 18:42 | `e46abf778d2c532883000d313c1ca383130389ba704266c5190ced7f2cfbfeb5` |
| Finnish | 1294386 | 2000-02-16 21:33 | `28edf6c8a50c9e5e5e1654135c95569b2220d195d1994313230f15b0f753c41b` |
| Swedish | 1294386 | 2000-02-08 15:48 | `d0f1fcab148839db56e9b0cc4df501c679b4a07ca889abde42c132185cece86b` |
| Dutch | 1294386 | 2000-02-11 16:44 | `acc7cfd3b5d0f9c67a291e15ff9c510ae3566782a074cf4f7c5432dc6ab0a99b` |
| Spanish | 1298482 | 2000-02-09 11:31 | `19e5b251aad700aefedc9da25ba169b54d6fcc8fd59daad6c19d3398cf343d6b` |

Compared with the reference copy, file by file:

- **Movies**: 18 of the 22 differ on every disc, 4 are the same everywhere.
  On the seven European discs the video stream of a sampled movie is the
  same and the audio stream is different on each, which is what dubbing
  gives; the audio was not listened to.
- **Sound effects**: of 266 files 163 are the same on every disc; the rest
  differ per language (speech).
- **Music**: 22 of 23 files are the same everywhere.
- **Level and graphics files** (`.ngn`): different on every disc.
- The 2003 English disc has another installer, an executable with the
  reference copy's link time but another size and hash, the reference
  copy's sound effects, and movies encoded again at 320×240.

German, Danish and Portuguese, which the installer has strings for, are not
among them. Only the reference build is supported: every address the
project knows (`crates/runtime/src/game.rs`, the analysis) belongs to it.
Supporting another language means analysing its executable, or carrying
its texts and files over to the reference build; that is recorded in the
backlog.

