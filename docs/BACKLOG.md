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
  kept, and takes the keyboard. Still to do: a canvas for the browser; a
  frame on Wayland where the compositor draws none. winit's own frame
  (`sctk-adwaita`, five subsurfaces around the window) makes WSLg's
  compositor (weston 9.0.0-215, WSLg 1.0.79) end with signal 11 about a
  second after it lists those subsurfaces as windows of the RDP shell, so
  the feature is off and the window there has no title bar. What is established
  (2026-11-20), with the compositor's log at its most detailed
  (`WESTON_RDP_DEBUG_LEVEL=5` in `%USERPROFILE%\.wslgconfig`):
  - it is not the game: `tools/wslg-frame-crash`, a window with that frame
    filled with one colour, brings the compositor down too;
  - the compositor's last act is the first update of one of the frame's
    sub-surfaces as a window of the RDP shell: it allocates the shared
    buffer, logs the copy it is about to make (888x44 pixels) and dies in
    `weston_surface_copy_content`; the sub-surface had been registered
    with "surface has no view";
  - it depends on timing: 3 runs in 3 on the software renderer, 1 in 3 on
    the graphics card, none in 6 while nothing else was going on. Giving
    the window a picture at once does not prevent it.
  So it is a fault of WSLg's compositor with sub-surfaces, to be reported
  to WSLg with the minimal program and the log. For the window: a frame of
  the project's own, or X11 on WSLg (which gets the Windows frame but
  needs the system library `libxkbcommon-x11`); full screen; the mouse and joysticks; trying it on
  Windows with a real graphics card.
- **One render pass per frame is not yet reached when the frame mixes
  copies and triangles**: a fill or copy between two batches ends the pass.
  Fills and copies could join the pass as commands.
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
  texture stage, wireframe and point fill, points and lines, depth bias,
  stencil, the texture factor, border addressing, and untransformed
  vertices passed straight to a draw call.
- **A run starts from what the previous one wrote.** `andy boot` keeps
  `out/run` (saves, `toy2.cfg`) between runs, as an installed game would,
  so two runs are identical only if that directory is. Tests and replays
  need a way to start from a named state.
- **The guest heap never reuses memory.** `Os::allocate` only moves forward;
  blocks the game frees, and the runtime's own blocks (surface memory,
  DIB sections, vertex buffers), are never returned. A long session will
  need a real allocator.
- **Movies are silent.** Their pictures are decoded and shown
  (`crates/mpeg`, ADR 0015); the Layer II audio decoder is not written yet
  and there is no sound output for it to feed. Only seeking to the start
  and synchronous `Update` are implemented.
- **Sound is not heard.** DirectSound buffers hold their samples and their
  play cursors advance, but nothing mixes them to an output yet.
- **Input comes only from recordings** (ADR 0017): keyboard, by file. No
  window supplies it live yet, and mouse and joystick have no events.
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
- **Italian on the reference build** (ADR 0016): find how the game looks
  its texts up, then bring the Italian disc's texts, movies, speech and
  graphics over as a catalogued modification, chosen by `LANGUAGE IT` in
  `toy2.ini` until the Options menu has it.
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
- **A settings file** for the screen mode and the project's own settings
  (ADR 0016).
- Reference screenshots of the original game, once the port draws.
- Which font Windows picks for the face name "ariel" (the game's spelling;
  no such font exists). The runtime answers with its Arial substitute.
- That `TEXTUREPERSPECTIVE` starts off under DirectX 6 (it changes nothing
  here: the adapter always corrects perspective, ADR 0014).
- What a texture stage without a texture reads as (white here), and what
  an `rhw` of zero or less does (taken as 1 here): ADR 0014.
- `IDirectDrawSurface4::Unlock` on a surface whose lock is held by `GetDC`
  returns `DDERR_NOTLOCKED` here; the real code is not known.
