# ts2-retold

Fan project: static recompilation (x86 → Rust) of the PC version of Toy Story 2
(the 1999 action game by Traveller's Tales), targeting the browser (WASM +
WebGPU) and native Windows, followed by catalogued enhancements and
multiplayer. Full brief: `docs/BRIEF.md` (amended by the ADRs).
Recon findings: `docs/RECON.md`. Decisions: `docs/adr/`.

**Names.** Display title: "Toy Story 2: Retold" (README heading). Repository,
directory and identifiers: `ts2-retold`. Recompiler codename: `andy` (ADR 0007):
binary `andy`, crates `andy-*`, prefix `andy_` in generated code.
License: Apache-2.0 (ADR 0005).

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
- **Quality and performance over speed of delivery.** No deadlines. Prefer the
  correct, measured solution to the quick one.
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
- Work on the Linux filesystem; the game lives on `/mnt/...` and is only read.
- Web target: build wasm in WSL, serve on localhost with COOP/COEP, open in
  Chrome on Windows.
- Remind the user to raise the WSL memory limit in `.wslconfig` before heavy
  parallel builds.
