# Draft of a report to WSLg: the compositor ends with signal 11 on a window with sub-surfaces

To be sent by the project's owner to `github.com/microsoft/wslg/issues`.
Not yet sent. Everything below was observed with the program in
`tools/wslg-frame-crash` of this repository.

---

**Title:** weston crashes (signal 11) in `weston_surface_copy_content` when a client-side-decorated window's sub-surfaces become RDP shell windows

**Environment**

- WSLg 1.0.79, weston 9.0.0-215 (other versions: to be filled in by the
  sender)
- A Wayland client that draws its own frame with five sub-surfaces
  around the window (winit 0.30 with its `sctk-adwaita` frame)

**What happens**

About a second after the compositor lists the frame's sub-surfaces as
windows of the RDP shell, it ends with signal 11 and every graphical
program of the session is gone. With the compositor's log at its most
detailed (`WESTON_RDP_DEBUG_LEVEL=5` in `%USERPROFILE%\.wslgconfig`), its
last act is the first update of one of the frame's sub-surfaces as a
window of the RDP shell: it allocates the shared buffer, logs the copy it
is about to make (888 by 44 pixels) and dies in
`weston_surface_copy_content`. That sub-surface had been registered with
"surface has no view".

When nothing crashes the same sub-surfaces are shown wrongly: the frame's
transparent borders, 44 pixels wide, appear as an opaque band around the
window.

**To reproduce**

```
git clone https://github.com/Byloth/ts2-retold
cd ts2-retold/tools/wslg-frame-crash
cargo run --release
```

A window with that frame, filled with one colour. It does not happen
every time: the program brought the compositor down in none of 8 runs at
one time and in 4 of 6 at another, the second time with someone working
at the Windows desktop. In the traces the crash follows the window being
activated.

**Ruled out**

- the version of wgpu (the same with wgpu 24);
- a pause in drawing, or the lack of a first picture;
- the particular program: a game window with the same frame brought the
  compositor down in 3 runs of 3 on the software renderer and in 5 of 11
  on the graphics card.

**Expected**

Sub-surfaces of a window shown as part of it, and no crash.

**Attached by the sender**

The compositor's log of a crashing run at debug level 5.
