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
  `DEFTEXT` lines. How localised releases used them, and whether they had
  their own executable, is not known (`UNVERIFIED`).
- **Leftovers of development** are on the disc: PlayStation executables
  (`data\SLES_020.67`, `SLUS_008.93`, `psx.exe`, `system.cnf`), `.bak` and
  `.btm` build scripts.
- **`audio\music21.wav` is not part of the game.** The installed copy has
  it; the disc does not. It is 0.2 seconds of silence (4484 zero bytes,
  11 kHz mono), dated 11 November 1999, the days of the files the 1999
  repackaging added (`regsetup.exe`, `DeIsL1.isu`). The executable names
  its music files and this name is not among them, nor anywhere else in it.
  It is taken to be an artefact of that repackaging.
