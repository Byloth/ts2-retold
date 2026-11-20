# ADR 0013 — Threads take turns deterministically

Status: Accepted (2026-11-20)

## Context

The game starts a second thread for streamed music and synchronises with it
through events, a semaphore and a critical section; its first thread even
busy-waits for the second to start. Host threads would make every run
different.

## Decision

- The program's threads run **one at a time**. A thread's turn ends when it
  waits, or after 50,000 instructions (one millisecond of the virtual clock).
  The order is round-robin. Nothing depends on the host's scheduler or clock,
  so a run repeats exactly.
- Waits with a timeout are measured on the virtual clock. When every thread
  is waiting, the clock jumps to the earliest timeout; if there is none, the
  machine stops and says so.
- A host function that must wait puts its thread to sleep and is simply
  **called again** when the wait is over; nothing is kept on the host's stack.
- A thread that must wait *inside a callback* cannot be set aside. The other
  threads then run on top of it until its wait ends. Only the innermost such
  thread can be resumed; if an outer one is what everybody waits for, the
  machine reports a deadlock rather than guessing.

## Consequences

- Deterministic replays (Phase 2) cover the game's own threads.
- Recompiled code, where the program's call stack is the host's stack, needs
  more: a real stack per thread (coroutines; in a browser, a transformation
  like Asyncify or worker threads) or a native replacement of the streaming
  thread. That decision is deferred to the recompiler.
