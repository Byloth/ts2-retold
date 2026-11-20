# ADR 0017: Input — one state, read by the game, recorded by reads

Status: accepted (2026-11-20)

## Context

The game takes its controls from DirectInput: once per frame it reads the
whole state of the keyboard with `GetDeviceState`. Phase 2 of the roadmap
(time trials) is built on replaying input, and phases 4 and 5 send it over
a network.

## Decision

1. **There is one input state** (`Os::input`: keys by DirectInput scan
   code, mouse buttons and motion). Whoever supplies input writes it — a
   recording now, a window later, the network after that — and the game
   reads it through its devices. Nothing reaches the game any other way:
   no synthetic window messages, no calls into the game.
2. **Recorded input is timed in reads of the keyboard**, not in
   milliseconds or instructions: "from the n-th read on, this key is down".
   The game samples once per frame and sees nothing of what happens between
   samples, so this is the whole of what it can observe, and a recording
   replays identically whatever the speed of the machine.
3. **The text form** is one change per line — read number, `press` or
   `release`, DirectInput key name — so that recordings can be written by
   hand, compared and kept with tests. `andy boot --input FILE` plays one.
4. Key names and codes come from `dinput.h` through `tools/key_table.py`.

## Consequences

- A recording made on one machine drives the same game states on another,
  provided the game is otherwise deterministic, which the virtual clock and
  the cooperative threads are there for.
- A recording belongs to a build and to a starting state (saves, settings):
  both have to be named when recordings are kept (see the backlog on the
  state a run starts from).
- The mouse and joysticks will need their own events when they are used;
  the game has shown no use of the mouse so far.
