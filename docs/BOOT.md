# Booting the game

`andy boot --trace` loads the executable into the interpreter and runs it
until something stops it, listing every call into the operating system.
Everything below was observed in that trace on 2026-11-20 (build
`pc-1999-10-29`), not inferred from disassembly.

```sh
cargo run --release --bin andy -- boot --trace
```

## How far it gets

**120,029,248 instructions and 85 system calls**, at about 58 million
instructions per second in the interpreter, then it stops at
`USER32 LoadIconA`: the program is about to register its window class.

| Phase | What happens |
|---|---|
| C runtime start | `GetVersion`, heap creation (small-block heap: `HeapAlloc` + `VirtualAlloc` reserve/commit), standard handles, command line, environment, code page and character tables |
| `WinMain` | Opens `HKLM\Software\TravellersTalesToyStory2`, reads `path` and `cdpath`, asks for `allow32bit` |
| Installation check | Opens `C:\TOY2\validate.tta` |
| Saves | Reads `Toy200.sav` and `Toy299.sav` |
| CPU benchmark | Ten timed runs of a floating-point loop (see below) |
| Settings | Looks for `c:\toy2.ini`, then `C:\TOY2\toy2.ini` (neither exists), reads `toy2.cfg` |
| Window | `LoadIconA` — not implemented yet |

## Findings

- **The game benchmarks the CPU at startup.** At `0x47EF90` it runs, ten
  times, a loop of 1,000,000 iterations of `fild / fdiv / fmul / fstp` and
  measures each run with `timeGetTime`. In the interpreter each run costs
  exactly 12,000,000 instructions. On a modern CPU such a run takes well under
  the timer's resolution. What the program does with the ten measurements is
  `UNVERIFIED`, but this is the obvious suspect for its timing problems on
  fast machines, and the first thing to trace for the fixed-timestep baseline.
- **The registry layout is confirmed**: key and value names as in ADR 0011.
- **An optional `toy2.ini`** is looked up in two places before `toy2.cfg`.
  The executable has a `LANGUAGE` setting read from an ini file (`RECON.md`);
  this is where it would come from. Its format is `UNVERIFIED`.
- **x87 precision.** By the time `WinMain` runs the control word is `0x023F`:
  double precision, round to nearest, as ADR 0010 assumes. The only
  computation under extended precision so far is the C runtime's Pentium
  FDIV-bug self-test at `0x4D0241`, which runs before the control word is set
  and whose result is only compared with a threshold.

## Next

The window and Direct3D layer: window class and message loop (the program's
window procedure is guest code the runtime must call back into), then the
DirectDraw / Direct3D 6 COM objects.
