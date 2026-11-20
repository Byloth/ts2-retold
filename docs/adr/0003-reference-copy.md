# ADR 0003 — Reference copy of the game

Status: Accepted, with an open action (2026-11-20)

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
What is still not done is a comparison with a verified dump of a pressed
disc (the archive's origin is unknown), and the list of regional builds:
the archive's languages are the installer's; whether the game itself has
other language builds is not known.

