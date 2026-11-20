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
