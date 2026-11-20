# ADR 0015: Movies — our own MPEG-1 decoder, no conversion

Status: accepted (2026-11-20, decided with the owner)

## Context

The game plays 22 movies through DirectShow's multimedia streams
(`IAMMultiMediaStream`), drawing each picture onto a DirectDraw surface it
then copies to the screen. The files are MPEG-1 system streams (video
320 pixels wide at about 30 pictures per second, Layer II audio at
44.1 kHz) stored as `rtlibs\*.dll`; `data\FMV\fmv.bin` only lists names.

The owner asked whether they should be converted at build time into a
format friendlier to the web.

## Decision

1. **The project has its own decoder**, `crates/mpeg` (`andy-mpeg`):
   demultiplexer, MPEG-1 video and Layer II audio in Rust, with no
   dependencies. It is a port of pl_mpeg (Dominic Szablewski, MIT), whose
   notice is kept in the crate; the standard's tables are converted from it
   by `tools/mpeg_tables.py`, not typed in.
2. **The movies are not converted.** The same files are decoded by the same
   code on every target.

## Why not convert

- The game wants pictures synchronously, one `Update` at a time, in a
  surface it reads and copies. A browser's `<video>` element and WebCodecs
  deliver frames asynchronously; they do not fit that without the machine
  being able to suspend.
- MPEG-1 at this size costs nothing to decode: the port does about 3000
  pictures per second on one core, a hundred times the need, and will be no
  problem in WebAssembly.
- Converting needs an encoder at build time (a large dependency, or an
  external tool the user must install) and then a second decoder for the
  new format on native targets.
- The decoder is integer arithmetic in a fixed order: identical pictures
  everywhere.

What conversion would buy is size: the movies are 218 MB, and a modern
codec would shrink them several times. That matters for a browser bundle
and is recorded in the backlog under build-time asset conversion; it would
be an optional step of the build tool, not a requirement.

## Verification

`andy movie FILE --yuv OUT` writes the decoded pictures; they are compared
with ffmpeg's. On `acti.dll` (300 pictures) and `l 03 bo.dll` (all 3196)
the picture counts are equal and the difference never falls below 52 dB:
the first pictures are identical and the rest differ by the rounding of
the inverse transform, which the standard leaves free within a tolerance.
