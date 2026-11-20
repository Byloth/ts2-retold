# ADR 0004 — Every deviation from the original is a catalogued modification

Status: Accepted in principle; mechanism to be designed in Phase 1 (2026-11-20)

## Decision

- With all modifications off, the port behaves like the original game.
- Any change in behaviour or presentation is a **named modification** with a
  stable identifier, a description, a rationale and an on/off switch. Nothing is
  changed silently.
- Each modification is classified, at minimum, as:
  - `fix` — corrects a defect of the original (e.g. frame-rate-dependent logic);
  - `gfx` — presentation only, no effect on simulation;
  - `gameplay` — changes simulation results;
  - `feature` — adds something new.
- Whether a modification affects the simulation is part of its metadata.
  Leaderboards (Phase 2) pin an exact set of simulation-affecting modifications
  as their ruleset.

## Amendment (2026-11-20): what the baseline is

- **A correct fixed timestep is part of the baseline**, not a modification. The
  original's behaviour on machines far faster than its target is a defect of
  the platform layer; the port never reproduces it.
- **Anything that is plainly a bug gets fixed.** Each such fix is still a
  catalogued `fix` entry, enabled by default, so that it stays documented and
  can be switched off to compare with the original.
- Whether something is "plainly a bug" or a design choice is decided case by
  case and recorded in the entry.

## Open points

- Where modifications attach (function replacement, hooks by original address,
  data overrides) and how the catalogue is stored.
