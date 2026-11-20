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
- **The GPU backend presents nowhere.** It renders off screen; `present`
  only records which surface is shown. A window (native) and a canvas
  (browser) are still to come.
- **One render pass per frame is not yet reached when the frame mixes
  copies and triangles**: a fill or copy between two batches ends the pass.
  Fills and copies could join the pass as commands.
- **Render targets are 8 bits per channel on the GPU** whatever the
  surface's format (ADR 0014); the original's 16-bit banding and dithering
  are not reproduced. A faithful path would quantise and dither in the
  shader and blend at 16 bits.
- **Draw states not implemented** stop the machine by name: fog, a second
  texture stage, wireframe and point fill, points and lines, depth bias,
  stencil, the texture factor, border addressing, `DrawPrimitiveVB` and
  untransformed vertices (Direct3D's transformation and lighting).
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
- **No input yet.** Nothing delivers keyboard, mouse or joystick events.
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
- **Other languages.** Eight language releases exist as separate builds
  with their own executable, dubbed movies and speech (ADR 0003). The
  project supports the English reference build only. To decide: analyse
  each executable, or bring a language's texts, movies and sounds to the
  reference build as a catalogued modification.
- Reference screenshots of the original game, once the port draws.
- Which font Windows picks for the face name "ariel" (the game's spelling;
  no such font exists). The runtime answers with its Arial substitute.
- That `TEXTUREPERSPECTIVE` starts off under DirectX 6 (it changes nothing
  here: the adapter always corrects perspective, ADR 0014).
- What a texture stage without a texture reads as (white here), and what
  an `rhw` of zero or less does (taken as 1 here): ADR 0014.
- `IDirectDrawSurface4::Unlock` on a surface whose lock is held by `GetDC`
  returns `DDERR_NOTLOCKED` here; the real code is not known.
