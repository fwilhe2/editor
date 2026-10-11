<!--
SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>

SPDX-License-Identifier: MIT
-->

# The browser shell — `ui_web/`, crate `quill-web`

Rust compiled to `wasm32-unknown-unknown`, bound to the page with `wasm-bindgen`; the document is
drawn into the DOM, one absolutely placed `div` per row on screen.

## The one design decision

**A page, not a window.** A browser tab has no menu bar, and inventing one makes a worse desktop
application. So there is one toolbar of common verbs — every button carries a command ID from the
core's table (`data-command="file.save"`) — and **Ctrl+K**, a palette over every `Command`. Keys
the browser keeps for itself (Ctrl+N, Ctrl+W, Ctrl+T) are not taken: New, Close and About are in the
toolbar and the palette. Cut, Copy and Paste are the browser's own `cut`/`copy`/`paste` events,
which carry the clipboard with no permission prompt. Save is a download; Open is the file picker;
unsaved changes are defended with `beforeunload`.

## Structure

| File | Holds |
|---|---|
| `src/keymap.rs` | `KeyboardEvent` values to actions, DOM-free; ⌘ on a Mac and Ctrl elsewhere resolved at the edge |
| `src/metrics.rs` | `Metrics` on a canvas, in the page's own computed font; a fallback width where there is no canvas |
| `src/lib.rs` | the page: rendering, the hidden `<textarea>` typing and the input method arrive through, the palette, the find bar |
| `index.html`, `style.css` | the elements, and the CSS — which turns kerning and ligatures off so a row is the sum of its characters, as it is measured |
| `build.sh` | `cargo build` + `wasm-bindgen`, refusing a CLI version that differs from `Cargo.lock`'s |
| `smoke.sh`, `smoke.js` | the real module against the real page in jsdom |

## Verification

`cargo test -p quill-web` (keys, metrics fallback), `ui_web/smoke.sh` (typing, undo, Enter, the
wheel, the palette, the find bar, New — 24 checks), and a real browser for how it looks
(`doc/ci.md`). The real browser found the bug jsdom could not: Firefox answers the computed `font`
shorthand with an empty string, so the canvas measured in its 10px default and nothing wrapped.

## Named gaps

- No inline input-method composition (the textarea commits as the method does).
- No scrollbar; the wheel and the keyboard scroll.
- A page cannot write back to the file it opened: Save is always a download.
