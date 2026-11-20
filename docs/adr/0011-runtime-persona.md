# ADR 0011 — What the runtime pretends to be

Status: Accepted (2026-11-20)

The runtime (`crates/runtime`) replaces Windows for the game. Its choices:

- **Windows 98.** `GetVersion` reports 4.10 of the 9x family, the system the
  game shipped for. The C runtime then takes its ANSI paths, which keeps the
  set of functions to implement small.
- **Installed in `C:\TOY2`.** The registry holds what the installer writes:
  `HKLM\Software\TravellersTalesToyStory2` with `path` = `C:\TOY2\data\`
  and `cdpath` = `C:\TOY2\` (the layout confirmed on an installed copy).
  The disc check (`validate.tta`) is satisfied from the latter. No
  `allow32bit` value: the game reads it, the installer does not write it.
- **The game directory is read-only.** Reads come from the user's copy;
  everything the program creates goes to a separate write directory that
  shadows it. Paths outside `C:\TOY2` do not exist. Names ignore case.
- **Time is virtual.** `timeGetTime` is a function of the number of
  instructions executed (50,000 per millisecond for now), so a run repeats
  exactly. How time advances once frames are presented is an open design
  point for the fixed-timestep work.
- **Imports are implemented when execution reaches them.** An unimplemented
  import stops the machine and names itself; nothing returns a made-up value.
- **Handlers use the guest calling convention** (arguments on the guest
  stack), so the same code serves the interpreter and recompiled code.
