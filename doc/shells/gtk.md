<!--
SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>

SPDX-License-Identifier: MIT
-->

# The GNOME shell — `ui_gtk/`, binary `quill-gtk`

GTK 4 and libadwaita, reached as `libadwaita::gtk`. Needs `libgtk-4-dev` and `libadwaita-1-dev`
to compile, which is why it is outside `default-members` and has its own CI job.

## Conventions

The GNOME HIG, not a sibling shell's: an `AdwHeaderBar` with Open on the left and a primary menu
(☰) on the right — no menu bar — a search bar that slides down under it, toasts for feedback, the
document drawn on the libadwaita palette in light and dark, and before discarding work an
`AdwAlertDialog` with Save suggested and Discard destructive. GNOME's accelerators: Ctrl+Shift+Z
redoes, Ctrl+W closes. Text arrives through `gtk::IMMulticontext`, so dead keys, compose and CJK
input methods work.

## Structure

| File | Holds |
|---|---|
| `args.rs` | `[FILE] [--render-to PNG] [--size WxH] [--dark]` |
| `menu.rs` | the primary menu's sections and GNOME's accelerators, as data |
| `keymap.rs` | editing keys (motion, deletion) to actions; text comes from the input method |
| `paint.rs` | `PangoMetrics` and `paint` — Pango on cairo, no GTK needed, so `--render-to` needs no display |
| `window.rs` | the window, its signals, the dialogs, the observer bridge |

## Verification

`cargo test -p quill-gtk` needs no display: menus (every command once, no accelerator twice, valid
action names), keys (every motion), metrics (cumulative advances, a tab four spaces wide), and two
renders of one document byte-identical with the dark one different. `artifacts.yml` renders the
sample twice with the release binary. The window itself: Broadway and a headless browser
(`doc/ci.md`).

## Named gaps

- The document font is a fixed `Monospace 11` rather than GNOME's monospace setting.
- The scrollbar measures in lines rather than wrapped rows, so its thumb is approximate in a document
  of long wrapped lines.
- No double-click to select a word.
