# ADR 0012 — The display adapter the game sees

Status: Accepted (2026-11-20), including the renderer order chosen by the owner
the same day.

## Decision

DirectDraw and Direct3D are implemented at the level of their COM interfaces,
as the game calls them, not by patching the game's renderer. What they report:

- **One adapter**, the primary display ("Primary Display Driver", `display`),
  with hardware 3D and 32 MB of video memory.
- **Display modes** 320×200, 320×240, 400×300, 512×384, 640×400, 640×480,
  800×600, 1024×768, 1152×864, 1280×1024 and 1600×1200, each at 8, 16 and 32
  bits per pixel; 16-bit is RGB 565. The low resolutions were ordinary modes
  on accelerators of the time and the game looks for 320×240 while examining
  the machine. Without
  `DDEDM_REFRESHRATES` the refresh rate is reported as 0, the adapter default.
- **One 3D device**, "Direct3D HAL": a rasteriser of transformed and lit
  vertices, RGB colour model, 16-bit z-buffer, textures up to 2048×2048 that
  need not be square, perspective correction, all filters, alpha blending
  with every factor, vertex and table fog. The exact capability words are in
  `crates/runtime/src/win32/d3d/caps.rs`.
- **No software 3D devices.** DirectX also listed its own software rasterisers
  ("Ramp", "RGB", "MMX"); their capability sets cannot be reproduced
  faithfully and nothing is gained by pretending.
- **DirectX 6.** The game examines the machine through `IDirectDraw2` and
  `IDirect3D2`, then creates its renderer through `IDirectDraw4` and
  `IDirect3D3`; all are offered on the same object. Interface identifiers of
  later DirectX versions are refused, as a DirectX 6 runtime would.
- **A 16-bit z-buffer** is the one depth format.
- **Texture formats**, in the order they are enumerated: RGB 565,
  ARGB 1555, ARGB 4444, XRGB 8888 and ARGB 8888. No palettised, luminance,
  bump or compressed formats. The game takes the first format that has the
  depth and alpha it wants, so the order is part of the persona; which
  order the cards of the time used is `UNVERIFIED`.

Rule: **a capability is claimed only if the renderer implements it.** The game
chooses its code paths from these words.

## Consequences

- Wider or modern resolutions, deeper z-buffers and the like are catalogued
  modifications on top of this baseline (ADR 0004), not part of it.
- The capability words are a contract for whatever renderer sits behind.

## The renderer: GPU first

The interfaces above end in two kinds of work: 2D surfaces the game locks and
writes with the CPU, and triangles drawn into the same surfaces. Two orders of
construction were considered: a deterministic software reference renderer
first with the GPU renderer validated against it, or the GPU renderer
directly. **The owner chose the GPU renderer directly** (`wgpu`, which serves
native targets and WebGPU from one code base).

What follows from it:

- Every surface has two homes: memory the game can lock, and a GPU texture.
  Each operation runs where the surface's current contents already are
  (a blit onto a surface last drawn by the GPU is a GPU blit), and contents
  move between the two only when an operation truly needs the other side.
  Transfers from the GPU back to memory are the expensive direction and are
  counted, so that a frame that needs one is visible in the statistics.
- Without a reference renderer, correctness is checked by rendering off
  screen, reading the image back and comparing: against images of the
  original game supplied by the owner, and against stored images for
  regression. Images depend on the GPU and driver; tests that compare images
  allow for that or run on one named software Vulkan driver.
- The runtime talks to the renderer through an interface of its own, so it
  can still be built and tested, and the game booted, with no GPU at all.

## 2026-11-20: which graphics adapter

The backend chooses its adapter itself: one that can draw to the window, a
real graphics card before a software renderer, then Vulkan, Direct3D 12,
Metal, OpenGL; `ANDY_BACKEND` names a backend to put first. The system's
default choice is not used, because under WSL it is Vulkan on a software
renderer while OpenGL (with `GALLIUM_DRIVER=d3d12`) reaches the graphics
card.

The backend therefore has to work on OpenGL as on the others, and two
things it did were not portable: copying depth textures to and from buffers
(now done through a texture of 16-bit numbers where the API cannot), and
taking a flat-shaded triangle's colour from "the first vertex" (now each
such triangle gets vertices of its own with that colour). The tests pass
on both.

