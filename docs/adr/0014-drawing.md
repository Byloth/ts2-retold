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

## How the game uses it (found 2026-11-20)

The game transforms through `IDirect3DVertexBuffer::ProcessVertices`, from
buffers of untransformed vertices into buffers of transformed ones, and
draws those with `DrawIndexedPrimitiveVB`. A buffer filled that way keeps,
beside what the program can see, each vertex's position before the
division by `w`, as Direct3D's own buffers did: drawn from it, a triangle
that reaches behind the viewer is clipped by the backend. A vertex the
program has changed since is drawn from the buffer's visible contents.

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
- **`rhw` that is zero or not a number** is taken as 1 (`UNVERIFIED`). A
  negative one is honoured: it is how Direct3D marks a vertex behind the
  viewer, and the game draws such vertices after reading them back from
  `ProcessVertices`.

## Consequences

- The backend stays small: surfaces, one pipeline family, no matrices, no
  lights. A second backend (a software one, another API) implements the
  same few operations.
- Transformation and lighting can be tested on their own, with `Headless`,
  against numbers.
- CPU cost per vertex is paid by the interpreter's host, not the guest; at
  the game's vertex counts it is negligible next to interpretation.

## Addendum (2026-11-21): the nearest texel is chosen in the shader

The game draws some surfaces from a single column of a texture, with
every vertex at the same `u`, exactly on the edge between two texels: the
band at the waist of the tin robot (`u` = 224/256), a patch of wall by the
pole in the first level. Interpolated across a triangle such a coordinate
carries a rounding error of either sign, and a sampler set to the nearest
texel takes one column or the other from pixel to pixel: the surface came
out as noise that changed with every frame, on every adapter tried.

When a texture is sampled at its nearest texel both enlarged and reduced,
the shader now rounds the coordinates to 1/256 of a texel, takes the texel
they fall in and samples its centre. Where exact arithmetic puts a
coordinate on an edge, the texel is the one exact arithmetic gives.

- This changes pixels wherever nearest sampling met an edge, the menus'
  texts included (their glyphs are drawn at a scale that puts many pixels
  on edges): each such pixel now takes a defined texel instead of whichever
  the adapter's arithmetic gave.
- `UNVERIFIED`: which of the two texels the graphics cards of 1999 took at
  an exact edge. They kept texture coordinates in fixed point, so their
  choice was steady; whether it was this one has no source at hand, and
  wants a picture of the original game on such a card.

## Addendum (2026-11-21): triangles at a finer grain (`render-scale`)

The owner found the picture stepped, and its edges jumping as the view
moves: the game draws at 640 by 480 and the window enlarges the finished
picture. With `render-scale` the backend draws triangles at N by N texels
to each pixel of their target (`Gpu::set_render_scale`; `--scale N`, 3 if
not given: 1920 by 1440).

- **A surface becomes finer the first time triangles are drawn on it**,
  and the depth surface beside it with it: its texture is replaced by one
  N times as wide and high, with what it held, each pixel a square of
  texels. Nothing else about the surface changes: its size, and every
  rectangle the program names, stay in its own pixels.
- **Rectangles stay in pixels.** Fills, copies and transfers are scaled to
  the texture they act on: a copy from a surface in pixels onto a finer one
  enlarges without smoothing (the menus' pictures keep their pixels), one
  from a finer surface takes one texel for each pixel, a transfer to memory
  (a lock) gives each pixel the mean of its texels, or its first texel for
  depth, and a transfer from memory writes each pixel as a square.
- **The runtime knows nothing of it**: vertices arrive in the clip square
  as before, and only the viewport is scaled.
- The window shows a finer picture smoothed (the original grain is shown
  with its pixels as pixels, as before).
- Measured in the first level on the RTX 4070 Ti through WSL: 363 frames a
  second at the original grain, 375 at three by three — no cost that can
  be told from noise. The simulation is the same to the instruction.
- `tools/check.sh` replays at the original grain: its outcomes are of the
  game's own picture.

Not done: multisampling (the finer grain is the anti-aliasing: at three by
three and a window up to 1440 lines every pixel shown comes from its own
texels or more); a grain chosen by the window's size; a picture shown at
less than half its grain would want more than one smoothed sample.
`UNVERIFIED` on the owner's display: seen so far only as frames read back
at 640 by 480, where edges come out averaged.
