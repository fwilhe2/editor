<!--
SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>

SPDX-License-Identifier: MIT
-->

# The Windows shell — `ui_win32/`, binary `quill-win32`

Win32 and GDI from Rust, through Microsoft's `windows` crate. **The `.exe` needs nothing Windows
does not ship**: system DLLs only, the C runtime linked statically (`.cargo/config.toml`), checked
on every build by reading the import table back. `doc/decisions/0001-win32-shell.md` is why Win32
rather than WinUI.

## Conventions

Notepad's: a system menu bar (File, Edit, Help) with mnemonics and accelerators in its labels,
Ctrl+Y to redo, F3 and Shift+F3 to find again, comdlg32's Open, Save As and **Find** dialogs (the
last modeless, as in Notepad), `*name - Quill` as the title, a three-button "Do you want to save
changes?" before work is lost, a vertical scroll bar, the **system caret** (so the user's blink rate
and assistive technology follow it), a dark title bar when the user's theme is dark, per-monitor
DPI v2, and `CF_UNICODETEXT` with CRLF on the clipboard.

## Structure

The portable half compiles and tests on any host; only `gdi.rs` and `win.rs` need Windows.

| File | Holds |
|---|---|
| `args.rs` | `[FILE] [--render-to BMP] [--size WxH] [--dark]` |
| `menu.rs` | the menu bar as data: commands, labels with `&` mnemonics, accelerators |
| `keymap.rs` | virtual keys to actions, `WM_CHAR`'s UTF-16 surrogate pairs, what a character types |
| `theme.rs` | the palette, the window's geometry, DPI scaling and wheel arithmetic |
| `gdi.rs` | fonts, `GdiMetrics` (`GetTextExtentExPointW`), painting with `ExtTextOutW` given those same advances, and `--render-to`'s DIB |
| `win.rs` | the window procedure, the dialogs, the clipboard, the observer bridge |
| `build.rs`, `data/` | the icon, the version block and the application manifest |

`win.rs` takes every modal call (message box, file dialog) **between** two short borrows of the
window state, never inside one, because a nested message loop re-enters the window procedure.

## Verification

From Linux: `cargo test -p quill-win32`, and `check`/`clippy`/`doc` with
`--target x86_64-pc-windows-msvc`. Linked: `cargo xwin build`, then Wine — `--render-to` headless,
the window under Xvfb (`doc/ci.md`, `scripts/run.sh win32`). On `windows-latest`: the
`cfg(windows)` tests, the release `.exe`'s import table, stack reserve, version block, icon and
manifest, and two renders byte-identical with `--dark` different.

## Named gaps

- GDI does not shape: no ligatures, no complex scripts (Arabic, Devanagari).
- No inline IME composition; the IME's own window composes.
- The follow-the-system accent colour and Mica are not used.
