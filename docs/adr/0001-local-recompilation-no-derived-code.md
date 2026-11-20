# ADR 0001 — Local recompilation; no assets or derived code in the repo

Status: Accepted (2026-11-20)

## Context

The recompiler's output is a translation of a copyrighted executable. Shipping
it would be distributing a derived work, exactly like shipping assets.

## Decision

- The repository contains only original code: the recompiler, the runtime
  (OS/DirectX replacement layer), tools and documentation.
- Anyone with their own copy of the game runs the recompiler locally. Its output
  lives in git-ignored directories and is never committed or published.
- Published builds, if any, must not contain recompiled code or assets. A
  browser-hosted build therefore needs in-browser recompilation; the recompiler
  is kept free of host-only dependencies so that it can later be compiled to
  WASM. That step is not scheduled yet.

## Consequences

- No "download and play" release until in-browser recompilation exists.
- Patches and mods must be expressed against the original binary (addresses,
  function identities), not as edits to generated code.

## Amendment (2026-11-20): one local build tool, several targets

The owner clarified the product: **one tool, run locally**. The user points it
at their own copy of the game and chooses how to play: a native build for
Windows or Linux, or a bundle that runs in a browser. Recompilation and the
conversion of assets both happen then, on the user's machine. The browser
runs only the finished result.

This replaces the earlier idea of recompiling inside the browser:

- The recompiler is a native tool and does not need to compile to WASM. It
  may use native libraries, for instance to convert audio and video.
- Assets are converted **at build time** into formats suited to the target
  (see `docs/BACKLOG.md`), not in the browser.
- A web bundle contains recompiled game code and game data. It is for the
  person who built it; putting it on a public site would be distributing the
  game, which rule 1 forbids. What the project publishes remains the tool.
