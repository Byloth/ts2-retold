# Project brief: browser port + multiplayer of a 1999 PC 3D platformer

Talk to me in Italian. Code, comments, commit messages and docs are in English.

## What we are building

A fan project built on the **PC version (1999) of Traveller's Tales' Toy Story 2 platformer**, sold on PC as *"Disney/Pixar's Action Game, Toy Story 2"*. I own the PC version.

The goal is not a 1:1 port. It starts as a clean, faithful port that runs in the browser, then gets new features: asynchronous multiplayer, new movement abilities, and eventually co-op with many Buzz Lightyears in the same world. In the film, Buzz meets a whole store aisle of copies of himself, so many Buzzes in one world fits the setting.

The working title is undecided. The candidates are **"Unboxed"** and **"Retold"**. Use a neutral codename (`ts2-port`) in code until I decide.

## What I already have

- In another Claude Code session I statically recompiled a different PC game (x86, DX9) and got it running with acceptable performance. If I bring that pipeline into this repo, **study it before writing a new one**. Don't assume what it contains: ask me.
- Environment: **WSL2, CLI only (no Linux GUI), Rust already installed. No Visual Studio.**
  - Recompiler, codegen, builds and headless tests all run in WSL2.
  - Web target: build wasm in WSL, serve on localhost (with COOP/COEP headers) and open Chrome on Windows.
  - Native Windows build: cross-compile to `x86_64-pc-windows-gnu` with mingw-w64 and launch the `.exe` from WSL through interop. Use `cargo-xwin` only if a dependency really needs MSVC.
  - Work on the Linux filesystem, never on `/mnt/c`. Only read the game files from there.
  - Remind me to raise WSL's memory limit in `.wslconfig` before heavy parallel builds.

## Non-negotiable legal constraints

The IP belongs to Disney/Pixar and Activision, two very aggressive rights holders. I know the risk and I accept it. These rules keep the project defensible:

1. **No game assets in the repo or in any build output.** No executables, data files, textures, audio, box art or logos. Users load **their own game files** in the browser (File System Access API, or upload into OPFS).
2. **No monetization** of any kind: no ads, no paid tiers, no donations tied to the game.
3. **No "Toy Story", "Disney" or "Pixar" in the project name, domain, logo or branding.** Factual compatibility statements ("requires your own copy of…") are fine.
4. Check the license of every dependency or tool before adopting it. Some recompilation frameworks are noncommercial-only, which is fine here but must be recorded.

## Roadmap (strict order: don't start a phase before the previous one passes its gate)

**Phase 0 — Reconnaissance (this session).** Find out what we're dealing with before writing any code:
- Executable: size, hash, PE sections, imports, entry point, compiler/linker fingerprints.
- **Copy protection** (SafeDisc, SecuROM or similar): is there any, and is the code encrypted? This can block static recompilation outright, so report it first.
- **Graphics API:** expect Direct3D 6/7 (or DirectDraw/software), not DX9. Find out exactly what it uses and how much of my existing graphics layer could be reused.
- Audio, input and timing APIs, CD checks, registry use, file I/O patterns.
- Data file formats: list the archives and files and guess their purpose. Don't reverse them deeply yet.
- Determinism risks: timing sources, RNG, floating-point/x87 use, frame-rate-dependent logic.
- Verify my memory that **most levels have timed races/challenges** (for example against RC). Find evidence in strings or data, or mark it unverified.

**Phase 1 — Faithful browser port.** WASM + WebGPU, user-supplied files, saves in OPFS. Keep it deterministic from day one: fixed timestep, controllable RNG seed, an input recording/playback hook at the input layer. Phase 2 depends on this.

**Phase 2 — Asynchronous time trials.** Per-level leaderboards. The client sends the **input recording**, not the time. The server replays it headless and checks the result, which gives anti-cheat almost for free. The same replays work as ghosts to race against.

**Phase 3 — "Falling with style" glide.** Buzz can glide from high places. It's the first real gameplay mod and the way into the movement/physics code. It needs one new animation.

**Phase 4 — Multi-Buzz presence.** Other players are visible in the same level, but no shared state. Like NFSU2's free roam.

**Phase 5 — Real co-op.** Settle an explicit authority model first: what is per-player (for example tokens) and what is shared and server-owned (for example enemies, triggers, bosses). Write it as an ADR before coding.

**Later / maybe:** other playable toys (needs new rigs, animations, movesets and level adjustments, so it's content production, not porting) and seamless physical transitions between levels instead of the level-select menu. **Out of scope:** a real open world and adapting the first film's story.

## Working rules

- Ask before any decision that's hard to reverse, or any deviation from this brief.
- Don't guess. If something can't be verified, mark it `UNVERIFIED` and move on.
- Record decisions as short ADRs in `docs/adr/`.
- Commit after each completed task with a clear message.
- No large downloads without asking.

## Deliverables for this session

1. Repo skeleton: `README.md` (legal rules included), `CLAUDE.md` with the non-negotiable rules above, `docs/adr/`, `.gitignore` that excludes any game files.
2. `docs/RECON.md` with the Phase 0 findings, each one marked verified or unverified.
3. A clear **go / no-go** on static recompilation of this executable, with the reasons, and if it's a no-go, the alternatives.
4. A wrap-up: what's done, open questions for me, and a proposed plan for the next session.

Start by asking me for the path to the game files, then go.
