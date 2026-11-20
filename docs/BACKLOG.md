# Backlog

Things decided but not scheduled, and gaps left open on purpose. Items move
out of here into the roadmap or an ADR when work on them starts.

## Features asked for by the owner

- **A "Video" entry in the game's Options menu** (the executable already has
  a `video options` string, whether as a working menu is `UNVERIFIED`):
  resolution, visual quality, windowed or full screen, and the other usual
  video settings. It replaces the start-up "Screen Mode Select" window, which
  the `skip-mode-select` modification currently answers with the game's
  defaults.
- **Native resolutions and 16:9.** Two routes, to be chosen on evidence once
  the game draws: rendering the game's 640×480 scene at the display's
  resolution (sharp 3D, 2D parts scaled up, 4:3), or running the game in a
  true high-resolution mode (needs its mode list extended and, for 16:9, its
  projection changed). The renderer is resolution-independent either way.

## Gaps in the runtime

- **GDI text is not drawn** (`crates/runtime/src/win32/gdi32.rs`). Needed to
  show the "Screen Mode Select" window and DirectDraw debug text. It needs a
  text engine and fonts that can be shipped; the window asks for Comic Sans
  MS, so a free look-alike would have to stand in.
- **Sound is not heard.** DirectSound buffers hold their samples and their
  play cursors advance, but nothing mixes them to an output yet.
- **No real input.** The only "key presses" are the ones the
  `skip-mode-select` modification feeds to its window.
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

- The registry values of an installed copy (`reg query
  HKLM\SOFTWARE\WOW6432Node\TravellersTalesToyStory2`): the runtime assumes
  `path` and `cdpath` both name the `data` directory.
- The executable's hash against an original disc (ADR 0003).
- Reference screenshots of the original game, once the port draws.
