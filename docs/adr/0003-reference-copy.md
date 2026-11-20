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
