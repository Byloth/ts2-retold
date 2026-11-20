# Modifications

Everything the port does differently from the original game (ADR 0004). The
source of truth is `crates/runtime/src/mods.rs`; `andy mods` prints the same
list. With every entry off the port behaves like the original.

| Identifier | Class | Affects simulation | On by default | What it does |
|---|---|---|---|---|
| `skip-mode-select` | feature | no | yes | The start-up "Screen Mode Select" window is not shown. Its three questions (display device, render method, screen resolution) are answered with the game's own defaults: the hardware 3D device and the first mode at least 480 lines high (640×480 at 16 bits). Choices saved in `toy2.cfg` are ignored, since they index lists that differ from one machine to another. |

## How `skip-mode-select` works

The game's routine runs unmodified. The runtime marks its window as hidden,
resets the saved driver index so that the routine takes its default path
(`crates/runtime/src/game.rs`), and delivers the space bar, the window's
"confirm" key, each time the window has drained its messages. Three
confirmations later the routine destroys the window and returns.

With the modification off the window would have to be shown, which needs GDI
text (`docs/BACKLOG.md`); until then the machine stops at the first text
output.

## Not modifications

Choices about the machine the game runs on are not listed here, because the
original game also met different machines: the display adapter (ADR 0012),
Windows 98 and the install location (ADR 0011), the virtual clock.
