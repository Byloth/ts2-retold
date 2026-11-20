# ADR 0012 — The display adapter the game sees

Status: Accepted (2026-11-20). The renderer behind it is still an open
decision (see the end).

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

## Open: the renderer

The interfaces above end in two kinds of work: 2D surfaces the game locks and
writes with the CPU, and triangles drawn into the same surfaces. Two orders of
construction are possible:

1. a **software reference renderer** first (exact Direct3D 6 rasterisation
   rules, deterministic, testable without a GPU, runs in the browser as is),
   then a GPU renderer validated against it, the way generated code is
   validated against the interpreter;
2. a **GPU renderer** first (`wgpu`, for native and WebGPU), with the
   CPU-written surfaces synchronised to it.

Either way the GPU renderer is the one that ships. The choice is the owner's.
