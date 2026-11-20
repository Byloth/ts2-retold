# ADR 0014: Drawing — transformation in the runtime, rasterisation in the backend

Status: accepted (2026-11-20)

## Context

The game draws through `IDirect3DDevice3`. It hands over two kinds of
vertices: ones it has transformed and lit itself (`D3DFVF_TLVERTEX`, screen
coordinates), and untransformed ones (position, normal, colour, one texture
coordinate set) that Direct3D transforms with the world, view and projection
matrices and lights with the viewport's lights and the current material.

Under DirectX 6 a hardware device rasterised only: transformation, lighting
and clipping ran on the CPU inside Direct3D, whatever the card. Hardware
transformation came with DirectX 7.

## Decision

1. **The video backend rasterises and nothing else.** The `Video` interface
   has one draw call. It takes a list of triangles whose vertices are in
   clip space with a diffuse colour, a specular colour and one pair of
   texture coordinates, and a `DrawState`: depth test and write, blending,
   alpha test, culling, flat or smooth shading, the specular add, the colour
   and alpha combiners of one texture stage, and the texture with its
   filters, addressing and colour key.
2. **Transformation and lighting belong to the runtime**, as they belonged
   to Direct3D. They are computed on the CPU in single precision, in a fixed
   order, so their results do not depend on the machine or the graphics
   card. This is what replays and network play need (phases 2, 4 and 5).
3. **Clipping is left to the backend.** Vertices go out in clip space, so
   the GPU clips; the runtime never cuts triangles. Vertices the program
   transformed itself are put back into clip space: the viewport maps the
   clip square onto its rectangle, `w` is `1 / rhw`, and half a pixel is
   added because a Direct3D screen coordinate names a pixel's centre while
   the viewport maps to pixel edges.
4. **Strips and fans are turned into lists in the runtime**, keeping each
   triangle's winding and its first vertex (which gives a flat-shaded
   triangle its colour). The backend sees one topology.
5. **The DirectX 5 render states for "the" texture act on stage 0**
   (`TEXTUREMAG`, `TEXTUREMIN`, `TEXTUREADDRESS*`, `TEXTUREMAPBLEND`), with
   the rules of Wine's DirectDraw (`dlls/ddraw/device.c`), which were
   established by tests on Windows: `TEXTUREMAPBLEND` rewrites the stage's
   operations; with "modulate" the alpha comes from the texture when it has
   an alpha channel; setting a stage state to a new value ends that mode.
   The game uses both ways of setting state.
6. **Nothing is approximated silently.** A state or value the draw path
   cannot express (fog, a second texture stage, wireframe, points and
   lines, …) stops the machine and names it, like an unimplemented function.
   Capabilities are added when the game is seen to use them.
7. **`Headless` does not rasterise.** It fills, copies and presents
   surfaces, counts triangles and keeps the last batch. Tests of the layers
   above the backend use it; pictures need the GPU backend.

## The adapter's own behaviour

- **Perspective correction is always on.** `TEXTUREPERSPECTIVE` starts off
  under DirectX 6 and the game never sets it, yet on the accelerators of
  the time its textures were corrected, because those cards corrected
  whatever the state said. The adapter of ADR 0012 does the same: the state
  is stored and not looked at. (That it starts off is from the DirectX
  documentation as remembered: `UNVERIFIED`.)
- **Specular highlights start off** on an `IDirect3DDevice3`, and a device
  starts as if `TEXTUREMAPBLEND` had been set to "modulate": both from
  Wine's DirectDraw. The game's first quad carries an uninitialised
  specular colour in one vertex, which shows at once if this is wrong.

## Known differences from a card of 1999

- **Colour depth.** A GPU backend keeps render targets at 8 bits per
  channel whatever the surface's format; a 16-bit target is reduced to 16
  bits only when the program reads it back. The original dithered or banded
  at 16 bits. `DITHERENABLE` therefore has no effect. A faithful 16-bit path
  is in the backlog.
- **A texture stage without a texture** reads as opaque white
  (`UNVERIFIED`; what cards of the time returned is not documented).
- **`rhw` that is zero, negative or not a number** is taken as 1
  (`UNVERIFIED`).

## Consequences

- The backend stays small: surfaces, one pipeline family, no matrices, no
  lights. A second backend (a software one, another API) implements the
  same few operations.
- Transformation and lighting can be tested on their own, with `Headless`,
  against numbers.
- CPU cost per vertex is paid by the interpreter's host, not the guest; at
  the game's vertex counts it is negligible next to interpretation.
