# ADR 0022: Things drawn from farther away

Status: accepted (2026-11-21) for scenery and coins; enemies await the owner

## Context

The owner sees objects appear only when near and wants them drawn from
farther away, now that the machines can afford it. Read from the game's
code (`docs/CODE-DISCOVERY.md`, "Distances"; first level only), there is
no one distance but four mechanisms, all from the camera, in units of
which the camera keeps 1200 from the character:

- **Scenery and the objects drawn with it**: two layers, a detailed one up
  to a depth and a simplified one from a depth, and a distance all round;
  6500, 5000 and 10000 at the default detail, from a table of three rows
  (the game's own "high detail" has 12000, 10000, 12000). Drawing only.
- **Coins and pickup marks**: drawn within about 6650, fading in; the loop
  that picks coins up asks whether the coin was drawn.
- **Enemies and characters**: one bit says that an object is seen (its
  room is drawn, it is within four times a range of its type, it is in
  view), and that bit decides both whether it is drawn and whether its
  script runs — which is its animation as well as its thinking.
- **The world**: a far plane at 48000.

## Decision

`draw-distance` multiplies the distances that decide drawing alone, by 3
unless `--distance N` says otherwise, never beyond the far plane:

1. **Scenery**: the function that sets the frame's limits from the detail's
   row (`0x4CDD10`) is replaced by one that sets the two depths and the
   distance all round times the factor. The near planes are the row's.
2. **Coins**: in the draw loop the test (`0x4413A4`) passes within the
   limit times the factor, with the original's fade at the new edge; what
   the original's own test would have said is remembered for each coin,
   and the pickup loop's question (`0x4A11CC`) asks for that too. A coin
   is picked up exactly where the original picks it up.
3. **Enemies are not touched.** Drawn from farther they would stand still
   until the original's rule wakes them, because one script moves and
   thinks; waking them from farther changes the game. Which of the two, or
   neither, is the owner's to decide.

## Consequences

- **It is not free on the machine's clock.** More is drawn by the game's
  own code, the clock is the count of instructions (50,000 to the
  millisecond), and the first level already takes about 36 ms of it a
  frame: with the factor at 3 a recording of 94 seconds shows 3022 frames
  where it showed 3207. The game steps by the time a frame took, so it is
  the same game at a lower frame rate — but a recording, timed in frames,
  no longer plays the same. `tools/check.sh` replays without the
  modification.
- The depth buffer has 16 bits: far surfaces close to each other may
  flicker where they did not reach before. Not seen yet; `UNVERIFIED`.
- `UNVERIFIED`: other readers of the coins' "was drawn" bit than the
  pickup loop; levels other than the first; which of the things the owner
  misses are scenery objects and which are enemies.
