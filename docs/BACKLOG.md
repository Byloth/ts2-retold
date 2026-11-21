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
  reading of the source, not a finding. To be reported to WSLg. The game
  starts its sound before its window, so on WSLg it stalls once at every
  start; the speaker's watch (ADR 0019) brings it back within seconds.
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
- **Italian** (ADR 0016), the modification `language-italian`: a hybrid on
  the reference build, made by transplanting from the Italian executable
  through an address map between the two builds (`andy pair`: 33,023 of
  33,186 relocation sites in the code aligned, 1,521 of 1,522 elsewhere,
  8,623 addresses paired, 4 with more than one answer, 40 without).
  Done: the texts, place by place (330 places, some 300 texts): the title,
  the main menu, the level names and the levels' own texts come out
  Italian. Still to do, in order:
  - the functions that differ (about 40), each looked at once and, if its
    difference is language, taken from the Italian executable: the four
    that draw glyphs first (accents, the hyphen, the colon and the
    narrower spacing; where the accents' glyphs come from in the data is
    not yet known), then the menus, whose texts in changed code are still
    English or stand against the wrong entry (the entry that loads a game
    reads as the Italian build's "load and save options");
  - speech, movies and screens served from the Italian disc, by a list of
    which files are language and which are the later revision's level
    data;
  - choosing the language in a settings file instead of `--with`;
  - a reading of all 300 pairs by someone who knows the game (the list is
    written by `andy pair --json`).
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
