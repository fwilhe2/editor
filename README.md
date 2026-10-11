<!--
SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>

SPDX-License-Identifier: MIT
-->

# editor — a template for shared-core, native-shell applications

One Rust core holds every rule and every piece of state; six front-ends draw it, each in its
platform's own idiom: a **CLI**, a **terminal** UI, a **GNOME** window (GTK 4 + libadwaita), a
**Windows** window (Win32 + GDI), a **macOS** window (AppKit + CoreText) and a **browser** page
(WebAssembly). All six are Rust; none of them owns a byte of the document.

The application is **Quill**, a plain text editor — small, but not a toy: soft wrapping decided in
the core, selection, a clipboard per platform, find, word-wise undo, files saved back byte for byte
with their BOM and line endings. It is here to be replaced. This repository is first of all a
**template**: the method, the checks that hold it, the CI that builds and packages for three
operating systems and the web, and the instructions for an agent to turn it into your application.

[![ci](https://github.com/fwilhe2/editor/actions/workflows/ci.yml/badge.svg)](https://github.com/fwilhe2/editor/actions/workflows/ci.yml)
[![shells](https://github.com/fwilhe2/editor/actions/workflows/shells.yml/badge.svg)](https://github.com/fwilhe2/editor/actions/workflows/shells.yml)
[![artifacts](https://github.com/fwilhe2/editor/actions/workflows/artifacts.yml/badge.svg)](https://github.com/fwilhe2/editor/actions/workflows/artifacts.yml)

## Why

Cross-platform UI toolkits work. Qt, GTK and wxWidgets have run one codebase on every desktop for
decades, and there are newer answers in the same spirit — Tauri if you are already writing Rust, or,
god forbid, Electron. Write the UI once, ship it everywhere. Nothing here disputes that this is
effective.

But native application UIs are special, and aesthetics matter. Every platform has its own paradigms:
where the menu bar lives, how a window is decorated, which shortcut redoes an edit, how an app asks
before discarding your work, what a toolbar is for. An app that honours those conventions feels
*delightful* — it belongs. An app that imports another platform's conventions always reads as a
visitor, no matter how good it is otherwise. That is the compromise cross-platform toolkits exist to
make, and it is a real one.

Historically the choice was economic rather than aesthetic. A genuinely native UI per platform means
a different language, toolkit, idiom and build system each time — Swift and SwiftUI, C# and WinUI,
Rust and GTK — plus the discipline to keep them all in step. Very few projects could justify paying
that three or four times over, so they either limited themselves to one platform or accepted the
compromise. The decision was made by the budget, not by what would be best for the user.

AI agents change that arithmetic. The per-platform work — learning each toolkit's shape, writing the
shell, keeping the build honest — is exactly the kind of labour that has become cheap, while the
part that still needs judgement (what the core owns, where the boundaries go, what each platform's
conventions actually are) stays small and human-sized. Native on every platform stops being a luxury
and becomes a normal choice, *provided* the architecture keeps the shells thin and the logic in one
place.

This repository exists to make that point concretely, with a real core, real bindings and real CI on
three operating systems — rather than as an argument. The editor is trivial on purpose; the shells
are the deliverable.

## Start an application from it

Hand an agent this repository and say what you want to build. The instructions are in
[`doc/bootstrap.md`](doc/bootstrap.md) (and, for Claude Code, the `bootstrap` skill). The
mechanical part is one command:

```sh
scripts/bootstrap.py notes --name Notes --app-id io.github.you.Notes --repo https://github.com/you/notes
scripts/bootstrap.py --check quill    # proves no trace of the template's name is left
```

CI runs exactly that on every push, and builds and tests the result, so the template cannot
quietly stop being one.

## How it fits together

```
                         core/  — every rule, every piece of state, no UI
                                          │
      ┌───────────┬──────────┬────────────┼────────────┬────────────┬────────────┐
     cli/      ui_tui/    ui_gtk/      ui_web/     ui_win32/     ui_mac/
   quill     quill-tui   quill-gtk    quill-web    quill-win32   quill-mac
   scripts   ratatui     GTK 4 +      wasm-bindgen Win32 + GDI   AppKit + CoreText
                         libadwaita   + the DOM    (windows-rs)  (objc2)
```

The rules the design leans on — [`doc/architecture.md`](doc/architecture.md) has them in full:

- **The core pushes; shells never poll.** An observer per shell, coalesced into one repaint.
- **Reads are windowed.** `App::view(frame)` returns only the rows that fit.
- **Undo lives in the core**, and an edit's inverse falls out of the data.
- **Layout lives in the core.** Up, Down, Home and End mean rows on screen, so the core wraps; a
  shell supplies only how wide a string is in its own unit, measured with the engine it draws with.
- **Whatever any window can do, the CLI can do** — enforced by a test that reads the core's API.
- **Verbs are data**: one table of commands; every shell's menus are tested against it.
- **Saving never makes a file worse**: an untouched save returns the bytes that were read.
- **Shells diverge**: a menu bar on Windows, a header bar on GNOME, the application menu and
  standard selectors on a Mac, a command palette in the browser.

| Path | What it is | Doc |
|---|---|---|
| `core/` | `quill-core` — document, history, layout, view, commands, `App` | [architecture](doc/architecture.md) |
| `cli/` | `quill` — every capability, scriptable; the parity and packaging ratchets | [parity](doc/cli-parity.md) |
| `ui_tui/` | `quill-tui` — ratatui; nano's conventions, Ctrl+P palette | [tui](doc/shells/tui.md) |
| `ui_gtk/` | `quill-gtk` — GTK 4 + libadwaita; the GNOME HIG | [gtk](doc/shells/gtk.md) |
| `ui_web/` | `quill-web` — wasm in a page; toolbar + Ctrl+K | [web](doc/shells/web.md) |
| `ui_win32/` | `quill-win32` — Win32 + GDI; an `.exe` needing nothing Windows lacks | [win32](doc/shells/win32.md) |
| `ui_mac/` | `quill-mac` — AppKit + CoreText via objc2; `Quill.app` | [mac](doc/shells/mac.md) |
| `examples/sample.sh` | a document built from every capability, through the CLI | |
| `scripts/` | `check.sh` (every local gate), `run.sh` (see a shell), `bootstrap.py` | |
| `doc/decisions/` | why Win32, why AppKit from Rust, why a template | |

## Building

A Rust toolchain (current stable) is enough for the core, the CLI, the terminal shell and the
portable halves of the Windows and macOS shells:

```sh
cargo test                                   # everything that needs no system package
cargo run -p quill-cli -- --help
cargo run -p quill-tui -- notes.txt
scripts/check.sh                             # every gate CI has that this machine can run
```

| Shell | Needs | Run |
|---|---|---|
| GNOME | `libgtk-4-dev libadwaita-1-dev` (or `gtk4-devel libadwaita-devel`) | `cargo run -p quill-gtk -- notes.txt` |
| browser | `wasm32-unknown-unknown`, `wasm-bindgen-cli` at `Cargo.lock`'s version | `scripts/run.sh web` |
| Windows | Windows and MSVC build tools — or, to look, `cargo-xwin` + Wine | `cargo run -p quill-win32 -- notes.txt` / `scripts/run.sh win32` |
| macOS | a Mac with the Xcode command-line tools | `cargo run -p quill-mac -- notes.txt` |

The Windows and macOS shells **type-check, lint and test their portable halves on any machine**,
because checking never links:

```sh
rustup target add x86_64-pc-windows-msvc aarch64-apple-darwin
cargo clippy -p quill-win32 --target x86_64-pc-windows-msvc --all-targets
cargo clippy -p quill-mac   --target aarch64-apple-darwin  --all-targets
```

Every GUI shell can draw a frame with no window: `quill-gtk notes.txt --render-to frame.png`. CI
draws each one twice and requires the bytes to match.

## The CLI

Everything a window can do, from a script. 1-based lines and columns; `--format json` for agents;
`--session FILE` to keep the caret, selection and undo history between invocations:

```sh
quill new notes.txt
quill --session s.json type notes.txt --text 'hello world'
quill --session s.json undo notes.txt               # removes " world" — undo goes a word at a time
quill view notes.txt --width 20                     # the rows a 20-column window would draw
quill --session s.json move notes.txt down --width 20
quill --format json info notes.txt
quill commands                                      # the verbs every window's menus are built from
```

## CI

Two tiers, and one release build per platform with everything about it as later steps of the same
job: debug checks in `ci.yml` and `shells.yml` (including the Windows and macOS shells checked from
Linux, and the bootstrap tested), and `artifacts.yml` for the `.deb`/`.rpm` packages, the Windows
`.exe` with its import table read back, a universal signed `Quill.app` in a DMG, and the web bundle —
each with its frames rendered twice and compared. [`doc/ci.md`](doc/ci.md).

## Status

Known gaps are listed per shell in `doc/shells/`. The largest: **the macOS shell has been
type-checked, linted and run in CI but not yet used interactively** on a Mac; no shell holds an input
method's inline composition; GDI does not shape complex scripts.

## License

MIT — see [LICENSE](LICENSE). REUSE-compliant: every file says who wrote it and under what terms.
