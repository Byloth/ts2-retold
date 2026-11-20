# Booting the game

`andy boot --trace` loads the executable into the interpreter and runs it
until something stops it, listing every call into the operating system.
Everything below was observed in that trace on 2026-11-20 (build
`pc-1999-10-29`), not inferred from disassembly.

```sh
cargo run --release --bin andy -- boot --trace
```

## How far it gets

**About 120.4 million instructions and 730 host calls**, at about 55 million
instructions per second in the interpreter, then it stops at
`DSOUND DirectSoundCreate`: graphics hardware has been examined and chosen,
the sound driver enumerated; creating the sound device is next.

| Phase | What happens |
|---|---|
| C runtime start | `GetVersion`, heap creation (small-block heap: `HeapAlloc` + `VirtualAlloc` reserve/commit), standard handles, command line, environment, code page and character tables |
| `WinMain` | Opens `HKLM\Software\TravellersTalesToyStory2`, reads `path` and `cdpath`, asks for `allow32bit` |
| Installation check | Opens `C:\TOY2\validate.tta` |
| Saves | Reads `Toy200.sav` and `Toy299.sav` |
| CPU benchmark | `CalculateClockSpeed`: ten timed runs of a floating-point loop |
| Settings | Looks for `toy2.ini`, reads `toy2.cfg` |
| "Examine machine" | Creates a helper window, enumerates DirectDraw drivers, creates DirectDraw, sets the cooperative level, reads capabilities and video memory, enumerates 18 display modes, obtains `IDirect3D2`, enumerates 3D devices |
| "Select drivers" | Picks the primary driver, the hardware 3D device, full screen |
| Disc | Reads `cd.txt` (base CD audio track) |
| Sound | Enumerates sound drivers; `DirectSoundCreate` — not implemented yet |

## The game's own log

Started with `-log` on its command line (`andy boot --args "-log"`), the game
writes `toy2.log`, and `andy boot` prints it. It is the best witness of what
the game makes of the runtime:

```
CalculateClockSpeed->time[0]=240 ... ->avgtime=240 ->CPUSPEED=233000000
INIT : CPU clock speed is 233.000000 MHZ
DIRECT DRAW DEVICE BASE NAME : display.
DIRECT DRAW DEVICE 3D SUPPORT : TRUE.
DIRECT 3D DEVICE BASE NAME : Direct3D HAL.
DIRECT 3D DEVICE TextureFlag : TRUE.   ZBufferFlag : TRUE.
DIRECT 3D DEVICE HardwareAccel : TRUE. SquareOnly : FALSE.  Alpha blending is ON.
INFO Direct3D - Hardware driver >>>Direct3D HAL, ...<<< selected
INFO Fullscreen mode is TRUE.
INFO RenderMode set to Direct3d render mode.
INFO Screen mode 640x480x8 selected.
CONFIG : Base CD track is 2.
```

## Findings

- **The game measures the CPU's speed at startup and we now know the result
  it computes.** `CalculateClockSpeed` at `0x47EF90` runs, ten times, a loop
  of 1,000,000 iterations of `fild / fdiv / fmul / fstp`, times each run with
  `timeGetTime`, averages, and derives a clock speed in hertz: with the
  runtime's virtual clock (240 ms per run) it reports **233 MHz**. On a
  modern CPU a run takes less than the timer's resolution. What the engine
  does with `CPUSPEED` is `UNVERIFIED`, but this is the obvious suspect for
  the timing problems on fast machines, and since the clock is virtual here,
  the runtime decides what the game measures.
- **Command-line switches**, separated by ` -`: `log`, `cheat`, `mpeg`,
  `demo`, `full`, `high` (meaning of the last four `UNVERIFIED`).
- **`toy2.ini` keywords**: `LOG`, `NOMPEG`, `HIMPEG`, `CHEAT`, `QUIET`,
  `MESSAGE`, `LANGUAGE`, `DEFTEXT`, `DEFKEY`, `JOY`, `KEY`, plus key names
  (`FIRE`, `JUMP`, `CAMLEFT`…). The file is looked up in `c:\`, then in the
  install directory; how the game then reads it is `UNVERIFIED` (it reopens
  it under a `\cd\` path).
- **The game selected a 640×480 mode at 8 bits per pixel.** Whether that is
  its real default, a consequence of this copy's `toy2.cfg` (written under a
  third-party wrapper with different modes), or of a capability it expected
  is `UNVERIFIED` and is the first thing to settle next.
- **`allow32bit` is the game's own registry option**, not an invention of the
  community fix.
- **The registry layout is confirmed**: key and value names as in ADR 0011.
- **x87 precision.** By the time `WinMain` runs the control word is `0x023F`:
  double precision, round to nearest, as ADR 0010 assumes. The only
  computation under extended precision so far is the C runtime's Pentium
  FDIV-bug self-test at `0x4D0241`, which runs before the control word is set
  and whose result is only compared with a threshold.
- **Windows.** The game first creates a helper window ("Examining Machine"),
  an ordinary overlapped window; its real window ("Toy2", 640×480 pop-up,
  window procedure at `0x4A6D40`, built on the DirectX SDK's `D3DApp` sample
  framework) comes later.

## Next

1. DirectSound objects and buffers (the game sets a cooperative level, creates
   buffers and stops one during start-up), then DirectInput.
2. Why the game selected an 8-bit mode.
3. The game's main window, `SetDisplayMode`, the primary surface with its
   back buffer and z-buffer, the 3D device: the first work for the GPU
   renderer (ADR 0012).
