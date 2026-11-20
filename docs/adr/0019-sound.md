# 0019. Sound: mixed in the runtime, heard through a receiver

Date: 2026-11-20. Status: accepted.

## Context

The game plays sound through DirectSound: 61 effects loaded at start-up
as secondary buffers (16 kHz, 16 bits, one channel), each duplicated five
times so that six can sound at once, and music streamed into looping
buffers that a thread of the game keeps refilling as position
notifications arrive. It sets volume, pan and frequency on every buffer.
Until now the buffers held their samples and their play cursors followed
the clock, but nothing was heard.

## Decision

**The runtime mixes; a receiver outside it makes the sound heard.** This
is the split ADR 0014 made for drawing. `win32::dsound` mixes what the
buffers play into one stream (`Machine::mix_sound`) and hands it to
`Machine::audio`, an `audio::Audio`. The command-line tool has two
receivers: a WAVE file (`andy boot --sound FILE`) and the sound device
(`andy play`).

**The mixed stream** is 44,100 frames a second, two channels, 16 bits:
`OUTPUT_RATE` frames for every second of the machine's clock, whatever
the computer does. It is mixed as the clock advances (at every turn of
the scheduler, a millisecond at most) and before anything about a buffer
changes, and the samples are read from the program's memory when the
mixer reaches them: that is what makes a refilled looping buffer sound
right, and it makes the stream a function of the run alone, the same on
every machine (`andy boot --sound` is deterministic).

**The mixer** keeps its own place in each playing buffer, exact to a
fraction of a sample (the play cursor the program is told goes in whole
milliseconds). Per buffer: rate conversion from the buffer's frequency to
the stream's by a straight line between neighbouring samples; volume and
pan as DirectSound defines them, attenuations in hundredths of a decibel,
a pan to one side attenuating the other; one channel played on both.
The sum is cut off at the limits of 16 bits.

**The primary buffer's format does not lower the quality.** DirectSound's
default primary format is 22 kHz, 8 bits, and with the drivers of 1999
(VxD) a program that did not change it was mixed at that; with the later
WDM drivers the system's mixer ignored it. The game has not been seen to
set it (`UNVERIFIED` beyond the first level). The stream is what a WDM
system gave: the card's own format.

**The sound device** (`crates/cli/src/speaker.rs`) is reached through
the PulseAudio server with the `pulseaudio` crate, which speaks the
server's protocol itself: no system library, where `cpal` needs ALSA's
development files installed (ADR 0006 avoids system-wide installs).
PipeWire and WSLg provide the same server. Other systems have no sound
device yet (`docs/BACKLOG.md`).

**Two clocks.** The machine's clock is kept in step with the computer's;
the sound device runs on its own. Between them is a queue of 60 ms, and
the device is fed from it up to half a percent faster or slower, by the
queue's length, so that the queue neither runs dry nor grows: the usual
answer in emulators (dynamic rate control). A dry queue gives silence
until it has filled again; more than 200 ms queued is dropped down to the
target, as sound that would come late.

**A device that stops taking sound** is noticed (less than a quarter of
the due sound taken over half a second) and its stream is paused and
started again. This exists because of a fault of WSLg, described in
`docs/BACKLOG.md` with a minimal program (`tools/wslg-audio-stall`); the
watch itself assumes nothing about WSLg.

## Consequences

- Effects and music are heard. Movies stay silent until the Layer II
  decoder exists (ADR 0015).
- `--sound` gives a file that can be compared between runs and builds.
- The straight-line rate conversion is one of the things the cards of the
  time did; which the original setup did is `UNVERIFIED`.
