<!--
SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>

SPDX-License-Identifier: MIT
-->

# The terminal shell — `ui_tui/`, binary `quill-tui`

ratatui over crossterm (reached as `ratatui::crossterm`, so the two cannot drift). The cheapest
shell with a real event loop, and the first built after the CLI.

## Conventions

nano's, which every terminal editor since has kept: Ctrl+letter commands, a help line at the
bottom listing the common ones (`^S Save  ^Q Close …`), the status line above it, the terminal's own
cursor as the caret. **Ctrl+P** opens a palette over every `Command`, ranked by
`quill_core::command::score`. Save As is **Alt+S**, because most terminals send Ctrl+Shift+S as
Ctrl+S. Discarding unsaved changes takes the same key twice — the terminal's Save/Discard dialog.
The mouse clicks, drags to select, and scrolls.

## Structure

| File | Holds |
|---|---|
| `main.rs` | the terminal's setup and teardown, on every exit path including a panic |
| `keymap.rs` | keys to actions, and `BINDINGS`, the command keys as data |
| `cells.rs` | `Metrics` in terminal cells (`unicode-width`) — its whole layout contribution |
| `shell.rs` | input routing, the prompt and palette, drawing from `App::view` |

## Verification

`cargo test -p quill-tui`: the keymap (every command has a key, every motion has a key), the cell
widths (what is drawn is as wide as what was measured), and the shell itself drawn into ratatui's
`TestBackend` — typing, wrapping, Down across wrapped rows, the register, the double-press close,
find, the palette, selection rendering and clicks. `ui_tui/drive.py` drives the real binary in a pty.

## Named gaps

- The clipboard is a register inside the process. OSC 52 would reach the system clipboard in the
  terminals that support it.
- No horizontal scrolling is needed (rows wrap), but a line wider than the terminal *and* without a
  break opportunity is broken between characters, as in every shell.
