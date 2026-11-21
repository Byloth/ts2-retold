# 0018. The window draws its own frame

Date: 2026-11-20. Status: accepted.

## Context

`andy play` opens a window. On Wayland a compositor may draw no frame
around it, and the program has to. winit's answer (`sctk-adwaita`) puts
the frame in five sub-surfaces around the window; WSLg's compositor shows
those wrongly and at times dies of them (`docs/BACKLOG.md`), which takes
every window of the desktop session with it. Running without any frame
leaves a window that cannot be moved, resized or closed with the pointer.

## Decision

The window asks the window system for no frame and draws one itself, as
part of its own picture, on every system:

- `crates/cli/src/frame.rs` holds the frame: its geometry (what a point
  belongs to: the bar, one of three buttons, an edge or corner, the
  picture) and the bar drawn as pixels. It has no window and no graphics
  adapter in it, so it is tested as plain data.
- The presenter (`crates/gpu/src/window.rs`) draws the bar across the top,
  the border along the other sides and the game's picture in the room
  that is left, in the one render pass that shows a frame.
- The window loop (`crates/cli/src/play.rs`) turns the pointer into
  requests to the window system: moving (a press on the bar), resizing (a
  press on an edge), minimising, maximising (also a double click on the
  bar), closing, and the system's window menu (right button on the bar).
  Moving and resizing themselves are done by the window system.

## Its look

A clear sky with small white clouds, overcast while the window does not
have the keyboard; the title at the left in the bundled typeface. The
drawing is the project's own, made by the program: it calls the film to
mind and uses nothing of the rights holders' (no logo, no lettering of
the logo, no character, no artwork), as rule 3 of `CLAUDE.md` requires.
The clouds have fixed places counted from the left, so they do not move
while the window changes width.

## Consequences

- One look and one behaviour on every system, and nothing that depends on
  sub-surfaces.
- What a system's own frame gives for free is not there: on Windows,
  snapping to the screen's edges and the window's shadow. To be looked at
  when the native Windows target is tried (`docs/BACKLOG.md`).
- The corners are square: round ones need a window with transparency,
  which WSLg shows opaque.

## The gauge (2026-11-22)

Asked for by the owner: **F9 shows and hides a small panel in the top
right corner of the game's picture** that says how fast the game is being
shown.

- It is the window's own, like the frame: drawn by the window over the
  game's picture (`Presenter::set_overlay`), never by the game, and F9
  does not reach the game. (The reference's default keys do not use F9;
  a player who gave F9 to an action in the game would lose it here.)
- It reads, twice a second: the frames of the game shown in a second, the
  time from one frame to the next, and the frames a second there could
  be if nothing were waited for: the frames shown divided by the time
  spent running the machine and showing them. The game presents at the
  display's 60 a second (ADR 0014); the third figure is the room left.
- Its look is a space ranger's wrist panel, drawn by code
  (`crates/cli/src/gauge.rs`): a white shell with a purple rim, a lime
  strip with three oval buttons, a dark glass with green figures of seven
  strokes, a band of red and white stripes. No picture, name or sign of
  the rights holders is used (rule 3).
- `andy play --gauge` starts with it shown, and `andy play` prints the
  last reading when it ends.
- Seen as a picture (`cargo test -p andy gauge -- --ignored` writes
  `out/gauge.png`); on the window itself it is `UNVERIFIED` by the tool's
  author, who cannot see the Windows desktop.

