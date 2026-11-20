# ADR 0012 — The display adapter the game sees

Status: Accepted (2026-11-20), including the renderer order chosen by the owner
the same day.

## Decision

DirectDraw and Direct3D are implemented at the level of their COM interfaces,
as the game calls them, not by patching the game's renderer. What they report:

- **One adapter**, the primary display ("Primary Display Driver", `display`),
  with hardware 3D and 32 MB of video memory.
- **Display modes** 640×480, 800×600, 1024×768, 1152×864, 1280×1024 and
  1600×1200, each at 8, 16 and 32 bits per pixel; 16-bit is RGB 565. Without
  `DDEDM_REFRESHRATES` the refresh rate is reported as 0, the adapter default.
- **One 3D device**, "Direct3D HAL": a rasteriser of transformed and lit
  vertices, RGB colour model, 16-bit z-buffer, textures up to 2048×2048 that
  need not be square, perspective correction, all filters, alpha blending
  with every factor, vertex and table fog. The exact capability words are in
  `crates/runtime/src/win32/d3d/caps.rs`.
- **No software 3D devices.** DirectX also listed its own software rasterisers
  ("Ramp", "RGB", "MMX"); their capability sets cannot be reproduced
  faithfully and nothing is gained by pretending.
- **Interfaces**: `IDirectDraw`, `IDirectDraw2` and `IDirect3D2` on the same
  object. Other interface identifiers are refused, as a DirectX 5 runtime
  would refuse what it does not know.

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
