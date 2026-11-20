# ts2-retold

Fan project: static recompilation (x86 → Rust) of the PC version of Toy Story 2
(the 1999 action game by Traveller's Tales), targeting the browser (WASM +
WebGPU) and native Windows, followed by catalogued enhancements and
multiplayer. Full brief: `docs/BRIEF.md` (amended by the ADRs).
Recon findings: `docs/RECON.md`. Decisions: `docs/adr/`. Deferred work and
known gaps: `docs/BACKLOG.md`. The catalogue of modifications:
`docs/MODIFICATIONS.md` (keep it in step with `crates/runtime/src/mods.rs`). Analysis results:
`docs/CODE-DISCOVERY.md`. Instruction semantics and their verification:
`docs/SEMANTICS.md`. How drawing is split between runtime and video backend:
ADR 0014. Languages and settings (one build, English and Italian): ADR 0016.

**Crates.** `crates/cli` (`andy`): the command-line tool.
`crates/runtime` (`andy-runtime`): the machine, the loader and the replacement
for the Windows API and DirectX (ADR 0011, ADR 0012); how far the game boots is
in `docs/BOOT.md`. DirectX layouts are checked against the headers in the
git-ignored `ref/dx` (see `docs/LICENSES.md`), never copied from them.
`crates/gpu` (`andy-gpu`): the video backend on the GPU (`wgpu`), behind the
runtime's `Video` interface; its tests need a graphics adapter, a software
Vulkan driver (lavapipe) will do.
`crates/mpeg` (`andy-mpeg`): the decoder of the game's movies (ADR 0015).
`crates/recomp` (`andy-recomp`): PE loader, code discovery, test generation.
`crates/cpu` (`andy-cpu`): instruction semantics written once over a backend
trait, plus the interpreter backend (ADR 0009) and the x87 model (ADR 0010).
Every new instruction or change in `crates/cpu` must pass the differential
test below with zero differences.

**Names.** Display title: "Toy Story 2: Retold" (README heading). Repository,
directory and identifiers: `ts2-retold`. Recompiler codename: `andy` (ADR 0007):
binary `andy`, crates `andy-*`, prefix `andy_` in generated code.
License: Apache-2.0 (ADR 0005).

**Product shape (ADR 0001, amended).** One tool run locally: the user gives it
their own copy and picks a target (native Windows or Linux, or a browser
bundle). Recompilation and asset conversion happen at build time on the
user's machine; the browser only runs the finished result.

Talk to the user in **Italian**. Code, comments, commit messages and docs are in **English**.

## Non-negotiable rules

1. **No game assets or derived code in the repo or in any published build.**
   No executables, data files, textures, audio, box art, logos, and no recompiler
   output (it is a translation of the copyrighted executable). Users recompile
   locally from **their own copy**. `.gitignore` enforces this; check `git status`
   before every commit.
2. **No monetization**: no ads, no paid tiers, nothing sold, nothing gated.
   Voluntary donations for the developer's work are allowed, but a donation
   never grants anything: no access, builds, features, early releases or perks
   (ADR 0008).
3. **No "Toy Story", "Disney" or "Pixar" in the repository name, package names,
   code identifiers, domain or logo.** The README carries the display title
   "Toy Story 2: Retold" and says plainly which game this is, always together
   with the trademark notice and the statement that the project is unofficial
   and needs the user's own copy (ADR 0008). Never use the rights holders'
   logos or artwork.
4. **Check and record the license** of every dependency or tool before adopting it
   (`docs/LICENSES.md`).
5. Game files are **read-only**, read from the path in `TS2_GAME_DIR`. Never write
   there, never copy them into the repo.

## Working rules

- Ask before any decision that is hard to reverse or deviates from the brief.
- Don't guess. Mark anything not checked as `UNVERIFIED`.
- Record decisions as short ADRs in `docs/adr/`.
- Commit after each completed task with a clear message.
- No large downloads without asking.
- Roadmap phases are strictly ordered; each has a gate (see `docs/BRIEF.md`).
- **Quality and performance come first; this is the owner's most important
  requirement.** No deadlines. No cheap solutions, shortcuts, workarounds or
  quick-and-dirty code: model the real behaviour, measure, and do it properly
  even when it takes much longer. A host function either implements the
  documented behaviour or stops the machine naming what is missing; it never
  returns a made-up value to get past a call.
- **Constants come from the reference, not from memory.** Error codes, flag
  values, structure offsets and method orders are checked against the headers
  in `ref/dx` (or another named source) when they are written. Several
  Direct3D error codes first written from memory were wrong.
- **No workarounds, ever.** The cleanest mechanism is the only acceptable one.
  The example to remember: the start-up mode-select window was first passed
  by feeding it synthetic key presses; the owner rejected that outright, and
  it became a native replacement of the game's function (ADR 0004). When a
  shortcut suggests itself, build the mechanism it is avoiding.
- **Every deviation from the original game's behaviour is a named, catalogued,
  individually toggleable modification** (ADR 0004). The faithful baseline must
  always remain reachable by switching all of them off.

## Environment (ADR 0006)

- WSL2. Install tools per user or inside the project; Docker if system packages
  are needed. Avoid system-wide installs.
- **No WSL ↔ Windows interop.** Never try to launch `.exe` files from WSL. For
  anything that must run on Windows, give the user a script or an exact
  PowerShell command.
- mingw-w64 lives on the Windows side.
- Work on the Linux filesystem. The game is only read: the reference copy
  is under the git-ignored `ref/`; the owner's installed copy, with
  third-party fixes, is on `/mnt/e/Giochi/Toy Story 2` and is not the baseline.
- Web target: build wasm in WSL, serve on localhost with COOP/COEP, open in
  Chrome on Windows.
- Remind the user to raise the WSL memory limit in `.wslconfig` before heavy
  parallel builds.

## Commands

```sh
# The reference copy (ADR 0003): the disc's files in ref/disc, laid out as an
# installation by tools/game-dir-from-disc.sh. Both are git-ignored.
export TS2_GAME_DIR="$PWD/ref/game"
cargo test                                   # unit tests, no game files needed
cargo run --release --bin andy -- info       # layout of the executable
cargo run --release --bin andy -- analyze --json out/analysis.json

# Instruction semantics against Unicorn; rerun after every change to crates/cpu
cargo run --release --bin andy -- difftest --out out/difftest.jsonl --rounds 40
.venv/bin/python tools/difftest.py out/difftest.jsonl --jobs 24

# Run the game in the interpreter; lists host calls and where it stops.
# --args "-log" turns on the game's own log, printed at the end.
# Always give --limit: a looping game otherwise runs for 2 billion instructions.
cargo run --release --bin andy -- boot --trace --limit 400000000 --args "-log"
# The same on the graphics adapter, writing every tenth frame shown
cargo run --release --bin andy -- boot --limit 600000000 --gpu --frames out/frames --frame-step 10
```

`.venv` is created with `python3 -m venv .venv && .venv/bin/pip install unicorn==2.1.4`.

`out/` is git-ignored: everything in it is derived from the game.
