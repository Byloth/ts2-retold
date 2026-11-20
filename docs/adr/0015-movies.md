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

## The sound (2026-11-21)

The audio decoder (`crates/mpeg/src/audio.rs`) is the port of pl_mpeg's,
which follows kjmp2 by Martin J. Fiedler, with one deliberate difference.
The port was first made exact: its output equalled pl_mpeg's, compiled
from `ref/mpeg`, byte for byte on three movies. Compared with ffmpeg that
output was wrong by more than rounding: 10 to 35 dB below the signal, and
quieter by 2 to 9 percent overall. The cause is the requantisation:
a sample coded with `n` levels stands for `(2 * code - (n - 1)) / n`
(the standard's `C * (s + D)`), and pl_mpeg's integer arithmetic divides
by `n + 1`, making every subband quieter by `n / (n + 1)`, most where
the quantiser is coarsest, which is where a low bit rate puts most of
the sound. It also scales its output by 64/65. This decoder requantises
as the standard says, in floating point, and keeps pl_mpeg's tables,
frame reading and filter bank (the transform and the tables are
converted by `tools/mpeg_audio_tables.py`).

Verification: `andy movie FILE --pcm OUT` writes the decoded sound.
Against ffmpeg's floating-point decoder (`mp2float`) on `dlogo.dll`,
`acti.dll`, `l 01 in.dll` and `1st trailer.dll` (224, 128 and 64 kbit/s;
two channels and one) the frame counts are equal, the gain is 1.000000
and the difference is 115 to 122 dB below the signal in every frame.
A frame built by hand is decoded in the crate's tests.

The movies' sound is 44,100 samples a second, the rate of the runtime's
mix (ADR 0019), so it is played sample for sample: in a recording of the
boot the first movie's twelve seconds equal the decoder's output exactly.
The program does not ask for the sound; it adds the audio stream with
the default renderer, which here is the mixer, for as long as the stream
runs. Sound and pictures start from the same moment of the machine's
clock, shifted by the difference of their first time stamps (80 ms in
`dlogo.dll`, none in the others looked at).
