# Booting the game

`andy boot --trace` loads the executable into the interpreter and runs it
until something stops it, listing every call into the operating system.
Everything below was observed in that trace on 2026-11-20 (build
`pc-1999-10-29`), not inferred from disassembly.

```sh
cargo run --release --bin andy -- boot --trace
```

## How far it gets

**It no longer stops by itself.** After the movies and the title screen
("PRESS JUMP"), which fades out when nobody presses anything, the game
plays its demo: Andy's room in three dimensions, Buzz running, "DEMO MODE"
and the energy display on top, at about 4,500 triangles a frame.

**It shows 60 pictures a second and no more.** `Flip` takes place at the
display's next vertical blank and the next one waits for it, as DirectDraw
does with a display in exclusive mode; the game itself does not limit its
pace and relies on this. (Before this was modelled it showed about 190
pictures in a second of its clock, and hundreds of thousands at a screen
that only waits for a key.) Because the clock also passes while the game
waits, `andy boot` takes `--seconds` beside `--limit`: a screen waiting for
a key runs almost no instructions, and the instruction limit alone would
never end the run.

`andy play` runs the game in a window at the pace of its own clock, with
the keyboard as its input.

With recorded input (ADR 0017) it goes further: `--input` with a file
saying `3700 press SPACE` and `3706 release SPACE` presses "jump" at the
title screen and brings up the main menu (START GAME, OPTIONS, LOAD GAME,
MOVIE VIEWER, EXIT), which the game then draws for as long as it is left. A
second press chooses START GAME: the level map appears, drawn as an Etch
A Sketch, and the view flies to "ANDY'S HOUSE" through grey fog. A third
press (`5000 press SPACE`) enters the level: its introduction is played as a
movie, the loading screen ("LEVEL 1: ANDY'S HOUSE", "PRESS JUMP") waits,
and a fourth press (`7000 press SPACE`) starts **the first level**: Andy's
room, Buzz standing in it and turning his head, a toy helicopter flying
about, at some 3,500 triangles a frame (51 draw states in all by then).
Left alone for two minutes of its clock it goes on without stopping.
The level is played by the recording too: with the arrow keys and "jump"
held for stretches of reads Buzz runs under the desk, jumps (his shadow
stays on the floor), turns and opens his wings, and the camera follows.

The pictures look right as far as can be told without the original beside
them: the room, Buzz, an enemy robot, the lives, coins and battery
displays. (What was first taken for wrong colours on Buzz was that robot
standing against him.) Nothing has been compared with the original yet;
two things to check when it is: a bright vertical line at the edge of the
open door, and a black area beside it in some frames.

Before that it shows the trademark and rating screens with their fades,
plays its introduction movies through DirectShow (ADR 0015) and draws the
title screen: 16,824 triangles in 8,412 batches, two draw states.

With the `Headless` backend triangles are counted, not drawn (ADR 0014).
With `--gpu` they are drawn by the GPU backend (`crates/gpu`), and
`--frames out/frames --frame-step 10` writes what the game shows as PNG
files. Seen so far, on a software Vulkan driver: the trademark screen
fading in from black (frames 1 to 21), held, fading out (571 to 591); the
ESRB rating screen fading in (to 611), held, and out (1171 to 1181).
The fade is the full-screen quad: black, alpha-blended, its alpha changing
from frame to frame.

`andy boot --surfaces out/surfaces` writes every DirectDraw surface as a PNG
at the stop: the back buffer shows the trademark screen and the textures
their contents, so what the game has put on the surfaces so far is checked
by eye.

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
| Renderer start | Main window maximised, `DirectDrawCreate`, cooperative level `FULLSCREEN | EXCLUSIVE | ALLOWREBOOT | FPUSETUP`, `FindDevice`, `EnumZBufferFormats`, `SetDisplayMode(640, 480, 16)` |
| Surfaces and device | Primary surface with one back buffer, 16-bit z-buffer attached to it, `CreateDevice` (the x87 unit goes to single precision), a viewport covering the screen |
| Input | `DirectInputCreateA` (version 5), the keyboard with its data format, acquired; joysticks enumerated (none) |
| Render set-up | A material, three vertex buffers of 1000 vertices (two of transformed-and-lit vertices, one untransformed), render, light and texture-stage states |
| Debug font | `CreateFontA("ariel", 20)`, a 256×256 DIB section, each character measured and drawn with `TextOutA`, copied by hand into a 256×256 ARGB 1555 texture surface chosen through `EnumTextureFormats` |
| Lights | A second initialisation pass repeats the render set-up, then four lights are created and added to the viewport |
| Level 0 | Writes `Toy200.sav` and `toy2.cfg`, reads `rand.dat`, releases the duplicated sound buffers, loads `level00\level.raw` and `level00\level.ngn`; each texture is read as a bitmap into a DIB section (24-bit, or 8-bit with a colour table) and copied into a texture surface; three more materials |
| Backdrop | `glueSetBackdrop()`: a 640×480 off-screen surface filled from an 8-bit bitmap through `GetDIBColorTable`, then `BltFast` to the back buffer |
| Loading screen | Each frame: `Clear2`, `BeginScene`, the backdrop copied with `BltFast`, `DrawIndexedPrimitive` (a strip of four transformed-and-lit vertices: "LOADING…"), `EndScene`, `Flip` |
| Movies | `IDirectSound::Release`, `CoInitialize`, a multimedia stream per movie: `Initialize`, `AddMediaStream` for video and audio, `OpenFile` (`rtlibs\\acti.dll`, then the others of the introduction), `GetMediaStream`, `CreateSample`, `GetSurface`, `SetState`; per picture the sample's `Update`, `GetSurfaceDesc` and a stretching `Blt` to the back buffer, until `Update` reports the end |
| Title screen | DirectSound again, the title backdrop and "PRESS JUMP" drawn as textured quads. The music is streamed: a looping buffer with `IDirectSoundNotify` positions, refilled by the stream thread from `audio\*.wav` each time an event is signalled |
| Demo | The title fades out and the demo level is drawn. Per object: `SetTransform` for the world matrix, `ProcessVertices` from a buffer of untransformed vertices into one of transformed ones (transform; often with clip, sometimes with lighting), `DrawIndexedPrimitiveVB`. Lights are set every frame; `ZBIAS` is used |

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
- **Registry paths**, confirmed on the owner's installed copy: `path` is the
  `data` directory and `cdpath` the directory above it.
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

Playing the first level, in the window and with recordings that move
Buzz. A comparison with the original, when the owner can supply pictures
of it. Sound output with the Layer II audio decoder (movies are silent,
like all sound so far).
