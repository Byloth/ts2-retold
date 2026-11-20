# Toy Story 2: Retold

A fan-made static recompilation of **Toy Story 2**, the 1999 PC action game by
Traveller's Tales (sold on PC as *Disney/Pixar's Action Game, Toy Story 2*, on
consoles as *Toy Story 2: Buzz Lightyear to the Rescue*).

The goal is to run the game in the browser (WASM + WebGPU) and natively, then
build on it: asynchronous time trials, new movement abilities, graphical
enhancements and, eventually, co-op.

This is an unofficial fan project. It is not affiliated with, endorsed by or
sponsored by Disney, Pixar, Activision or Traveller's Tales.

**Status: early. An interpreter implements every instruction the game uses,
verified against an independent emulator on 1.8 million cases, and the game's
own code boots in it through its C runtime, installation check and settings,
up to the creation of its window. Nothing is playable yet.**

```sh
export TS2_GAME_DIR="/path/to/your/installed/game"
cargo run --release --bin andy -- analyze
```

## You need your own copy of the game

This repository contains **no game code and no game assets**, and neither will any
build published from it. The tools here read *your* legally owned copy of the PC
game and recompile it locally on your machine.

## Ground rules

1. No game assets or recompiled game code in this repository or in any release.
2. No monetization: no ads, no paid tiers, nothing sold, nothing gated.
   Voluntary donations support the developer's work on this open-source code
   and never grant anything in return: no access, builds, features or perks.
3. The project is not affiliated with, endorsed by or sponsored by the rights
   holders of the original game. Their trademarks are used only for factual
   compatibility statements.
4. Every third-party dependency's license is checked and recorded.

## Documentation

- [`docs/BRIEF.md`](docs/BRIEF.md) — project brief and roadmap
- [`docs/RECON.md`](docs/RECON.md) — Phase 0 findings on the executable and data
- [`docs/CODE-DISCOVERY.md`](docs/CODE-DISCOVERY.md) — what the recompiler's analysis finds in the executable
- [`docs/SEMANTICS.md`](docs/SEMANTICS.md) — instruction semantics and how they are verified
- [`docs/BOOT.md`](docs/BOOT.md) — how far the game currently boots
- [`docs/adr/`](docs/adr/) — architecture decision records

## Support

If you want to support the work on this code, you can do so through
[GitHub Sponsors](https://github.com/sponsors/Byloth). A donation is a thank-you
for the developer's time and nothing else: it does not buy or unlock anything.

## License

The code in this repository is licensed under the [Apache License 2.0](LICENSE).
This covers only the project's own code and grants no rights over the original
game. Third-party licenses are listed in [`docs/LICENSES.md`](docs/LICENSES.md).

## Trademarks and copyright

*Toy Story*, *Toy Story 2*, *Buzz Lightyear* and all related characters, names,
artwork and logos are trademarks and/or copyrighted works of Disney Enterprises,
Inc. and Pixar. The original game is © Disney/Pixar; it was developed by
Traveller's Tales and published by Activision. *Disney*, *Pixar*, *Activision*
and *Traveller's Tales* are trademarks of their respective owners.

They are named here only to identify the game this software works with. This
project is an unofficial fan work, not affiliated with, endorsed by or sponsored
by any of them, and it distributes none of their code or assets.
