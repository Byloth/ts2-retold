# Draft of a report to WSLg: sound stalls when a window appears while a stream is playing

To be sent by the project's owner to `github.com/microsoft/wslg/issues`.
Not yet sent. Everything below was measured with the program in
`tools/wslg-audio-stall` of this repository.

---

**Title:** PulseAudio playback stalls for every stream when a window is mapped while a stream is playing

**Environment**

- WSLg 1.0.79 (`wslg --version`: to be filled in by the sender, together
  with the versions of WSL, of the kernel and of Windows)
- Distribution: to be filled in
- Sound through WSLg's PulseAudio server (`PULSE_SERVER` as WSLg sets it)

**What happens**

A program opens a PulseAudio playback stream (44,100 Hz, 16 bits, stereo,
a target length of 40 ms, `adjust_latency`) and then opens a window. From
the moment the window is on the Windows desktop, the server asks the
stream for sound about once a second instead of about fifty times a
second, and it stays so. The same happens to streams of other programs
that are playing at that moment: it does not matter which program owns
the window or the stream. A stream that is started after the window is
on the screen is not affected. A stalled stream recovers when it is
paused (corked) and started again; the stalled server takes 2.5 to 4.5
seconds to carry out the pause.

**To reproduce**

```
git clone https://github.com/Byloth/ts2-retold
cd ts2-retold/tools/wslg-audio-stall
cargo run --release              # the sound first, then the window
cargo run --release -- late      # the window first, the sound four seconds later
cargo run --release -- recover   # as the first, with a pause and restart at five seconds
cargo run --release -- silent    # the window alone, while another program plays
```

The program plays silence and prints, for every second, how many times
the server asked the stream for data.

**Measured**

| | requests in each second |
|---|---|
| sound first, then the window | 51, 31, 1, 1, 1, ... |
| `late` | 50 throughout |
| `recover` | back to 50 after the pause and restart |

**Expected**

About fifty requests a second throughout, whatever windows appear.

**Notes**

Reading `module-rdp-sink.c` of WSLg's PulseAudio (not a finding, a
reading): the sink talks to the RDP side over a socket with blocking
reads, and resets its latency only when it goes idle, which would fit a
sink that falls behind once and never catches up while a stream stays
running.
