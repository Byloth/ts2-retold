# Backlog

Things decided but not scheduled, and gaps left open on purpose. Items move
out of here into the roadmap or an ADR when work on them starts.

## Features asked for by the owner

- **A "Video" entry in the game's Options menu** (the executable already has
  a `video options` string, whether as a working menu is `UNVERIFIED`):
  resolution, visual quality, windowed or full screen, and the other usual
  video settings. It takes over from the start-up "Screen Mode Select"
  window, which the `skip-mode-select` modification replaces with the game's
  default choice.
- **Native resolutions and 16:9.** Two routes, to be chosen on evidence once
  the game draws: rendering the game's 640×480 scene at the display's
  resolution (sharp 3D, 2D parts scaled up, 4:3), or running the game in a
  true high-resolution mode (needs its mode list extended and, for 16:9, its
  projection changed). The renderer is resolution-independent either way.

- **Lighter assets, converted at build time.** The build tool converts the
  user's files for the chosen target: music (159 MB of WAV) to a compressed
  format, videos (210 MB of MPEG-1, which browsers do not play) to a format
  the target decodes natively, and only the data files the game actually
  opens (the runtime logs every file opened; much of `data/` looks like
  developer leftovers). Conversion never happens in the browser (ADR 0001).

## Windows

- **Native Windows is written, compiled, and never run.** The whole tool
  type-checks for `x86_64-pc-windows-gnu`; the written game's Rust has
  nothing of Linux in it; the sound device there is through `cpal`
  (ADR 0019). `tools\windows\build.ps1` builds the tool, writes the
  game's code out and compiles it, on Windows, from a clone on a Windows
  drive. **To be run by the owner**, and what it shows fixed: the first
  run on Windows of the window's own frame (dragging, resizing), of the
  graphics adapter chosen there (Vulkan first, then Direct3D 12), of the
  sound, and of the paths.
- A program for Windows cannot be linked from WSL without mingw-w64
  installed there, which ADR 0006 keeps on the Windows side.

## Gaps in the runtime

- **GDI text has one typeface** (`crates/runtime/src/win32/text.rs`):
  Liberation Sans, regular, whatever face is asked for. Bold, italic,
  underline, rotation and the stock fonts stop the machine. The "Screen Mode
  Select" window asks for Comic Sans MS and uses stock fonts, so it cannot
  be shown with `skip-mode-select` off until those exist.
- **Text widths are not Arial's pixel-exact widths.** Windows used Arial's
  hand-tuned width at each size; the substitute rounds its scaled outline
  width. The game's font texture is laid out from these measurements, so
  its glyph cells can differ by a pixel from the original's.
- **GDI cannot draw on a DirectDraw surface.** `GetDC` gives a context that
  serves as the model for a compatible one (all the game does with it);
  drawing through it stops the machine.
- **Reading a surface back from the GPU blocks** (`Video::download`). A
  browser cannot block on it: the web target needs a way to suspend the
  machine there (JavaScript Promise Integration, or never needing the
  transfer). So far the game has caused no transfer back at all.
- **The window** (`andy play`) shows the picture, enlarged with its shape
  kept, and takes the keyboard, inside a frame the program draws itself (ADR 0018).
  Still to do: a canvas for the browser; full screen; the mouse and
  joysticks; trying it on Windows with a real graphics card, where the
  frame also has to be looked at again (snapping to the screen's edges
  and the window's shadow belong to the system's frame there); a
  comparison of the frame's bar with the pointer's real behaviour on a
  display with a scale other than 1 (`UNVERIFIED`). Why the frame is the
  program's own: winit's frame
  (`sctk-adwaita`, five subsurfaces around the window) makes WSLg's
  compositor (weston 9.0.0-215, WSLg 1.0.79) end with signal 11 about a
  second after it lists those subsurfaces as windows of the RDP shell, so
  the feature is off. What is established
  (2026-11-20), with the compositor's log at its most detailed
  (`WESTON_RDP_DEBUG_LEVEL=5` in `%USERPROFILE%\.wslgconfig`):
  - it is not the game: `tools/wslg-frame-crash`, a window with that frame
    filled with one colour, brings the compositor down too;
  - the compositor's last act is the first update of one of the frame's
    sub-surfaces as a window of the RDP shell: it allocates the shared
    buffer, logs the copy it is about to make (888x44 pixels) and dies in
    `weston_surface_copy_content`; the sub-surface had been registered
    with "surface has no view";
  - it comes and goes: with the frame the game brought it down in 3 runs
    of 3 on the software renderer and 5 of 11 on the graphics card; the
    minimal program in none of 8 at one time and 4 of 6 at another, with
    the owner working at the desktop the second time. In the traces it
    follows the window being activated;
  - it is not the version of wgpu (the minimal program built on wgpu 24
    brought it down too), nor a pause in drawing, nor the lack of a first
    picture;
  - the same sub-surfaces are also shown wrongly when nothing crashes: the
    frame's transparent borders, 44 pixels wide, appear as an opaque band
    around the window (black around the game, a flat colour around the
    test window).
  So it is a fault of WSLg's compositor with sub-surfaces, to be reported
  to WSLg with the minimal program and the log.
- **Speed** (ADR 0020 has the measures). In the first level the compiled
  game makes 482 frames a second without the graphics adapter and 374 on
  the graphics card as WSL reaches it; the interpreter made 85. What a
  frame's 2.7 ms now are, and what could be done about each:
  - 1.2 ms the game's code, 655 thousand instructions at 1.8 ns: blocks
    return to the machine's loop after every jump and call, and every
    access to memory looks its page up. Next: several blocks of a function
    as one piece of Rust with its jumps inside it, the registers held in
    variables across it; memory as one stretch of addresses;
  - 0.45 ms giving the frame's drawing to the graphics adapter, in one
    submission. Still to do within it: a fill or copy between two batches
    of triangles ends the render pass; they could join it as commands;
  - the game runs its whole simulation and drawing for every frame it
    presents, and presents at the display's 60 a second (`Flip` waits for
    the vertical blank, ADR 0014). **Showing more than 60 different
    frames a second** needs the drawing parted from the simulation: a
    modification to be designed once the game's own step is known (for
    the owner; not started).

- **Render targets are 8 bits per channel on the GPU** whatever the
  surface's format (ADR 0014); the original's 16-bit banding and dithering
  are not reproduced. A faithful path would quantise and dither in the
  shader and blend at 16 bits.
- **Direct3D's transformation and lighting are unverified against the
  original** (`d3d/vertex_buffer.rs`, `d3d/lighting.rs`): they follow
  Wine's model for devices older than DirectX 7. Open points: what
  `COLORVERTEX` replaces (here the material's diffuse colour only), what
  `ProcessVertices` writes for a vertex outside the view when asked to
  clip, and how far one unit of `ZBIAS` moves a triangle. Spot lights and
  parallel-point lights stop the machine.
- **Draw states not implemented** stop the machine by name: fog from a
  table (per pixel) and exponential or range vertex fog, a second
  texture stage, wireframe and point fill, points, depth bias,
  stencil, the texture factor and border addressing.
- **Untransformed vertices passed straight to a draw call** are
  transformed and lit by the path `ProcessVertices` uses (met in the second
  level, climbing the tree; replayed from the owner's recording of it).
- **Lines** (line lists and strips) are drawn as rectangles of triangles
  one pixel of the picture wide; which pixels Direct3D's own rule lights
  at a line's ends is not modelled. **Points** stop the machine.
- **A run starts from what the previous one wrote.** `andy boot` keeps
  `out/run` (saves, `toy2.cfg`) between runs, as an installed game would,
  so two runs are identical only if that directory is. `--state DIR`
  names another directory (an empty one is a game never played; ADR 0017);
  the older recordings of `tools/check.sh` still run from `out/run`.
- **The guest heap never reuses memory.** `Os::allocate` only moves forward;
  blocks the game frees, and the runtime's own blocks (surface memory,
  DIB sections, vertex buffers), are never returned. A long session will
  need a real allocator.
- **Movies** (ADR 0015): pictures and sound are decoded and played. Only
  seeking to the start and synchronous `Update` are implemented; an audio
  stream the program reads itself (without the default renderer) stops
  the machine by name. A picture that is late is delivered late, never
  dropped: DirectShow would drop pictures to stay with the clock
  (`UNVERIFIED` how), which matters only if the game falls behind, and on
  the machine's clock it does not (each of the opening movies takes
  exactly its length).
- **Sound** (ADR 0019) is mixed and heard, on Linux through a PulseAudio
  server. Still to do: a sound device on Windows and in the browser; a
  listening comparison with the original (rate conversion
  is a straight line between samples, the original setup's is
  `UNVERIFIED`); 3D sound and the other DirectSound interfaces stop the
  machine by name. When the game starts in the window its clock runs
  ahead of the computer's by about two seconds (a wait inside a callback
  cannot be bounded by the window's clock limit), so the window stands
  still for that long and that much silence is dropped.
- **WSLg's sound stalls when a window appears while sound plays** (WSLg
  1.0.79; found 2026-11-20). From the moment a window is on the screen the
  PulseAudio server asks every playing stream for sound about once a
  second instead of fifty times, whichever program owns the window or the
  stream, and it stays so until the sound has paused; a stream started
  after the window is well. `tools/wslg-audio-stall` shows it with a
  stream of silence and an empty window (requests in each second: 51, 31,
  1, 1, 1, ...; with `late` 50 throughout; with `recover` back to 50 after
  a pause that the stalled server takes 2.5 to 4.5 seconds to carry out).
  The cause inside WSLg is not known: its sink (`module-rdp-sink`, kept in
  `ref/wslg`) talks to the RDP side over a socket with blocking reads and
  resets its latency only when it goes idle, which fits, but that is a
  reading of the source, not a finding. To be reported to WSLg.
  The window is opened first and the sound device after it (decided by
  the owner, 2026-11-21: what plays the sound exists before the sound),
  and nothing watches for the stall or works around it. That order does
  not spare WSLg's fault: WSLg puts the window on the Windows desktop some
  time after every event the program gets (focus and the first redraw come
  within 30 ms), and a stream opened then still stalled in 5 starts of 9;
  opened 2 seconds later, in none of 2. So under WSLg, the development
  environment only, `andy play` is without sound for about every second
  start ("dropped for being late" then counts nearly the whole run), until
  WSLg is fixed.
- **The first 2.3 seconds of a run reach the sound device late, at once**
  (found 2026-11-22; they are silence, and the 2,341 ms a well run
  reports as dropped are these). The mixer is driven by what the program
  does about sound and by the scheduler's changes of thread; while the
  game loads, alone and without a sound, nothing drives it, and the first
  mix delivers all the time since the start. The fix is one line: mix
  whenever `Machine::run` returns, so that a device that plays as time
  passes is fed as time passes. **Tried and taken back, for the owner to
  decide**: with it the stream plays from the first moment, and WSLg's
  fault above then stalls the sound at every start (4 of 4) instead of
  about every second one; the late first delivery was what, by accident,
  let some starts through. On a system without that fault the fix is
  simply right.
- **Input** (ADR 0017) is the keyboard, from a recording or from the
  window; mouse and joystick have no events.
- **A pending flip holds back only the next flip.** On real drivers a lock
  or a blit on the surfaces being flipped could also wait or answer
  `DDERR_WASSTILLDRAWING` until the vertical blank; which did, and when, is
  `UNVERIFIED`, and the game has not been seen to depend on it. The picture
  is handed to the video backend when `Flip` is called, not at the blank.
  `WaitForVerticalBlank`, `GetScanLine` and `GetVerticalBlankStatus` stop
  the machine by name.
- **x87 state save/restore** (`fnsave`, `frstor`, `fnstenv`, `fldenv`) is not
  implemented (ADR 0010).
- **The streaming thread is run as the game wrote it**, by a scheduler that
  cannot resume a thread waiting inside a callback other than the innermost
  one (`crates/runtime/src/threads.rs`). Recompiled code will need either
  real stacks per thread or a native replacement of that thread.
- **`toy2.ini`**: the game finds it in the install directory but then looks
  for it again under a `\cd\` path; how it is meant to be read is not
  understood yet.

## To verify with the owner's help

- A verified dump of a pressed disc as a second witness for the reference
  copy in `ref/disc` (ADR 0003).
- **Languages** (ADR 0016), the modification `language`: Italian, French
  and Spanish on the reference build, each as data taken from the user's
  disc of that language through an address map between the two builds
  (`andy pair`). The owner's direction: a language is data only, one
  mechanism for all, no code of a language's own. Done for the three:
  the texts, place by place (330, 345 and 333 places); speech, movies and all textures read from the
  language's disc. **Next, in this order:**
  1. the typefaces: **done** (`one-typeface`). One atlas of each of the
     three (large, small, levels') put together from all the discs at
     hand, one table, and native functions in place of the reference's
     four (`0x49B580`, `0x49B2A0`, `0x49B3B0`, `0x49B630`) and of the
     glyph choice of a fifth (`0x49D390`, the large typeface in quads:
     the screens for loading and saving, a level's name); the Italian
     executable's functions and tables are gone, and every language runs
     the reference's code with the reference's timings. The boxes of the
     levels' messages (`0x49D390`, `0x49D910`) keep the game's own layout
     and draw through the replaced character function;
  2. texts that a language's build split over two lines are joined as
     the lines of one text (`Hybrid::texts_between`), and about thirty
     texts each language had left in English, in code that differs
     between the builds, are brought over by their order (the pause
     menu, the slots for saving, the hints). **Left in English**, listed
     at every start: in Italian and Spanish two hints of the screen
     where keys are chosen (the other build has five lines for the
     reference's three texts); in Italian "you need more tokens" and
     "jump to select" of the level map. Each needs reading the two
     builds' code at that place;
  3. the other places where something is put beside a text by a fixed
     number, to be found screen by screen and placed from the text's
     extent like the main menu's arrows.
  Known about the atlases:
  - what each disc's page holds was written down by looking at it
    (`typeface::PAGES`); a page not listed there (another pressing,
    another language) is not used and not replaced, and nothing says so
    yet when the game starts;
  - whether the French small page's cell after the ring is Æ or Œ is
    `UNVERIFIED`;
  - seen on screen with accents: the Spanish main menu and options (large
    and small), the Spanish pause menu (levels'). Not yet: a message box
    of a level, the screens for saving and for the keys;
  - the atlases are made at every start; a built game would make them
    once.
  Known about the texts, seen on the screen where keys are chosen:
  - in Italian and Spanish the second line of hints reads "pulsante
    desiderato" / "botón deseado" where English has "return to accept,
    esc to cancel": a place the two builds' code was aligned at though
    they draw other things there. With the two hints left in English
    (above), that screen's hints need the two builds read side by side;
  - the name of a long key ("barra spaziatrice") runs off the screen's
    right edge: it is drawn from a fixed place.
  Known about the layout, where the reference's suits English only:
  - in the Italian screen of the graphics settings two hints of two
    lines each stand on one another: the reference draws its two texts
    at the heights 178 and 186, the Italian build its four lines at 170,
    178, 186 and 194, and here a text of two lines is put around its one
    height (174 and 182, 182 and 190). No rule from one height gives the
    Italian build's four: in the options it puts the added line above
    (180, then the reference's 190). What would: reading, where the
    texts are joined, the heights the other build draws its lines at
    from its code, and carrying them with the text;
  - the pause menu's dark box is as wide as the English texts: the
    Spanish ones run past it;
  - a text of two lines stands around the height of the reference's one
    line: in the options the lower line comes close to the screen's
    edge.
  Still to do besides:
  - the comparison with each language's own game for every other screen:
    options, controls, graphics settings, loading and saving, the pause
    menu, the messages of the levels, the credits, the movie viewer;
  - choosing the language in a settings file instead of `--with`
    (**for the owner to decide**; proposed: the keyword `LANGUAGE` the
    game's `toy2.ini` already has, read by the tool from the directory
    the game writes to, picking among the discs at hand the one whose
    build is of that language);
  - a reading of all the text pairs by someone who knows the game
    (`andy pair --json`).
- **The first vertex of each sprite's quad has an unset specular colour**:
  the game leaves whatever was on its stack there (seen as a return
  address). Specular is off in those draws, so nothing shows; noted
  because the value differs from run to run of different code.
- **The other languages**: French, Spanish, Dutch, Swedish, Norwegian and
  Finnish are at hand as disc images; German, Danish and Portuguese have
  not been found (ADR 0003).
- **German from the Dreamcast release: measured, not done.** No German PC
  disc has been found. The German Dreamcast disc (examined 2026-11-20, not
  kept) is the same game on the same engine. Against the PC's assets:
  - texts: in its executable, in the PC's own markup — the same;
  - movies: MPEG-1 system streams (`.SFD`) at 320×416 and 3.6 Mbit/s with
    44.1 kHz stereo ADX audio, against the PC's 320×208 at 1.6 Mbit/s with
    64 kbit/s mono Layer II — better;
  - speech and effects: 16 kHz 4-bit ADPCM in sound banks (`.KAT`), against
    the PC's 16 kHz 16-bit PCM — worse.
  The owner's condition for using it is that nothing be worse than on the
  PC, so it waits for a German PC disc. The movies alone would be an
  improvement for any language, if their pictures are the same.
- **The settings file** (`retold.ini` in the state directory, ADR 0021)
  holds the mouse's and the camera's settings; the screen mode and the
  language (ADR 0016) are not in it yet.
- Reference screenshots of the original game, once the port draws.
- Which font Windows picks for the face name "ariel" (the game's spelling;
  no such font exists). The runtime answers with its Arial substitute.
- That `TEXTUREPERSPECTIVE` starts off under DirectX 6 (it changes nothing
  here: the adapter always corrects perspective, ADR 0014).
- What a texture stage without a texture reads as (white here), and what
  an `rhw` of zero or less does (taken as 1 here): ADR 0014.
- `IDirectDrawSurface4::Unlock` on a surface whose lock is held by `GetDC`
  returns `DDERR_NOTLOCKED` here; the real code is not known.
