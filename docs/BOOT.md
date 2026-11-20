# Booting the game

`andy boot --trace` loads the executable into the interpreter and runs it
until something stops it, listing every call into the operating system.
Everything below was observed in that trace on 2026-11-20 (build
`pc-1999-10-29`), not inferred from disassembly.

```sh
cargo run --release --bin andy -- boot --trace
```

## How far it gets

**About 131.6 million instructions**, then it stops at
`IDirectDraw4::SetDisplayMode(640, 480, 16)`: everything before the first
surface is done.

| Phase | What happens |
|---|---|
| C runtime start | `GetVersion`, heap creation (small-block heap: `HeapAlloc` + `VirtualAlloc` reserve/commit), standard handles, command line, environment, code page and character tables |
| `WinMain` | Opens `HKLM\Software\TravellersTalesToyStory2`, reads `path` and `cdpath`, asks for `allow32bit` |
| Installation check | Opens `validate.tta` under `cdpath` |
| Saves | Reads `Toy200.sav` and `Toy299.sav` |
| CPU benchmark | `CalculateClockSpeed`: ten timed runs of a floating-point loop |
| Settings | Looks for `toy2.ini`, reads `toy2.cfg` |
| "Examine machine" | Creates a helper window, enumerates DirectDraw drivers, creates DirectDraw, sets the cooperative level, reads capabilities and video memory, enumerates the display modes, obtains `IDirect3D2`, enumerates 3D devices |
| "Select drivers" | Picks the primary driver, the hardware 3D device, full screen |
| Disc | Reads `cd.txt` (base CD audio track) |
| Sound | Creates DirectSound, loads 61 sound effects from `sfx\*.wav` into buffers and duplicates them, starts the streaming thread |
| Second enumeration | `DirectDrawEnumerateExA`, `IDirectDraw4`, `IDirect3D3`: drivers, devices and modes again, DirectX 6 style |
| "Screen Mode Select" | The start-up window; skipped by a modification (`docs/MODIFICATIONS.md`) |
| Renderer start | Main window maximised, `DirectDrawCreate`, cooperative level `FULLSCREEN | EXCLUSIVE | ALLOWREBOOT | FPUSETUP`, `FindDevice`, `EnumZBufferFormats`, then `SetDisplayMode` — not implemented yet |

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
- **The game is a DirectX 6 program.** Its "examine machine" pass uses the
  DirectX 5 interfaces, but the renderer is created through `IDirectDraw4`
  and `IDirect3D3` by code that follows the DirectX 6 SDK's `D3DFrame`
  framework. With only DirectX 5 interfaces on offer it exits with **"Unable
  to enumerate a suitable device"** (`direct6.cpp`, line 103), the message
  known from modern PCs.
- **Mode selection.** While examining the machine the game looks for 320×240
  at 16 bits (a flag at `0x4E0690`, initialised to 1 in the executable), and
  falls back to the first mode listed. The mode it finally runs in comes from
  the "Screen Mode Select" window: saved indices from `toy2.cfg` if the saved
  driver index is not negative, otherwise the first hardware device and the
  first mode at least 480 lines high.
- **The registry paths name the `data` directory.** The game looks for
  `sfx\`, `cd.txt` and `validate.tta` under them, and in the copies in
  circulation those are in `data\` (`UNVERIFIED` against an installed copy's
  registry).
- **The game asks DirectDraw to set up the FPU** (`DDSCL_FPUSETUP`): Direct3D
  will switch the x87 unit to single precision, the case ADR 0010 prepares
  for.
- **A streaming thread** (at `0x437010`) is created during sound start-up and
  waits on three events; the first thread busy-waits until it has started
  (ADR 0013).
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

`SetDisplayMode`, the primary surface with its back buffer and z-buffer, the
3D device: the first work for the GPU renderer (ADR 0012). DirectInput is
created in the same stretch.
