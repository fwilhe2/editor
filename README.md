<!--
SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>

SPDX-License-Identifier: MIT
-->

# A Rust template for native applications

Build an application with one Rust core and a shell for each platform. The core owns the model,
behaviour, and application state. Shells translate platform input into core operations and render
the core's view using the conventions of their platform.

This repository is both a reusable template and a working example: **Quill**, a plain text editor.
It includes a CLI, terminal UI, GNOME desktop app, browser app, Windows app, and macOS app. All six
shells are written in Rust; none keeps a second copy of the document.

The pattern is especially useful for agent-driven development. Agents can implement a shell or
capability against clear interfaces, while the architecture keeps product behaviour in one place.
Tests and CI turn those interfaces into checks, so parallel implementation stays reviewable and
changes in one shell do not silently redefine the application.

[![ci](https://github.com/fwilhe2/editor/actions/workflows/ci.yml/badge.svg)](https://github.com/fwilhe2/editor/actions/workflows/ci.yml)
[![shells](https://github.com/fwilhe2/editor/actions/workflows/shells.yml/badge.svg)](https://github.com/fwilhe2/editor/actions/workflows/shells.yml)
[![artifacts](https://github.com/fwilhe2/editor/actions/workflows/artifacts.yml/badge.svg)](https://github.com/fwilhe2/editor/actions/workflows/artifacts.yml)

## The pattern in one picture

```
                core/   the model, every rule, all state — and no user interface
                  ▲
                  │  App::view(frame) → rows to paint      (the shell reads)
                  │  App::type_text, move_caret, undo, …   (the shell forwards input)
                  │  Observer::changed()                   (the core says "repaint")
                  │  Metrics: how wide is this text?       (the one question the core asks back)
                  │
   ┌────────┬─────┴────┬──────────┬───────────┬───────────┐
  cli/    ui_tui/    ui_gtk/    ui_web/    ui_win32/    ui_mac/
 scripts  terminal    GNOME     browser     Windows      macOS
```

A shell does four things and nothing else: it turns platform input into calls on `App`, hears that
something changed, asks for the rows that fit its window, and paints them. It holds no document
state, so six shells cannot disagree about what the application does — only about how it looks,
which is the point of having six.

## Understand it in ten minutes

The whole contract fits in one file with no toolkit. Run it, then read it:

```sh
cargo run -p quill-core --example minimal_shell
```

```
hello world              ← you type; the text goes to the core
  1 hello world          ← the shell prints the rows the core hands back
  2 |                      (| is the caret)
--- Untitled •
:up                      ← a motion, by the core's own name for it
  1 |hello world
  2
--- Untitled •
:+word-right             ← "+" extends the selection, as Shift does
  1 [hello] world
  2
--- Untitled •
:edit.undo               ← a verb from the core's command table
  1 |
--- Untitled
```

[`core/examples/minimal_shell.rs`](core/examples/minimal_shell.rs) is about 350 lines with its
comments and tests, and every real shell is that file plus its platform's conventions. From there:

| Next | Read |
|---|---|
| What happens when a key is pressed, function by function, in every shell | [doc/tour.md](doc/tour.md) |
| Where one capability lives in each layer, and which check notices when it is missing | [doc/tour.md](doc/tour.md#one-capability-top-to-bottom) |
| Why it is shaped this way: the ten rules, the traps, the checklists | [doc/architecture.md](doc/architecture.md) |
| Turning it into your own application | [doc/bootstrap.md](doc/bootstrap.md) |
| One platform's conventions and known gaps | [doc/shells/](doc/shells/) |

The code documents itself the same way throughout: every module opens with a design note saying
what it is for and what decision shaped it, deliberate shortcuts are marked `ponytail:` with the
way past them, and the rules that matter are tests — including one that fails when a document
names a file that no longer exists.

## Why this pattern

Cross-platform toolkits are a strong choice when a shared UI is the right product experience. A
different choice is to share the application model and behaviour while giving each platform its
own interface. That lets a GNOME app follow GNOME conventions, a Mac app behave like a Mac app, and
a browser app fit naturally into a page—all without implementing editing rules several times.

The tradeoff has traditionally been cost: each native shell brings its own APIs, event model, and
build environment. Agent-driven development changes the economics of that work. Agents are good at
repetitive, well-scoped implementation across platforms; people remain responsible for the product
model, boundaries, platform choices, and review. This template makes that division practical: a
shared core defines what the application does, and automated checks keep each shell connected to it.

The result is a pattern for building applications that are both **consistent in behaviour** and
**native in presentation**. Quill is deliberately a modest example, but it exercises the parts that
make the pattern useful: editing, selection, find, undo, platform clipboards, text layout, and
faithful file saving.

## What the example demonstrates

- **One application model.** The core owns document state, commands, history, and layout. Shells
  render a bounded view and forward input; toolkit text widgets do not become a competing source of
  truth.
- **A scriptable interface.** The CLI exposes application capabilities, including JSON output and
  optional session files. It is useful for automation, reproducible examples, and agent workflows.
- **Checks at the boundaries.** Tests check that core methods have CLI counterparts, CLI commands
  appear in the sample script, and commands and motions are represented in each shell's menus and
  key maps.
- **Platform-specific interfaces.** GTK/libadwaita, Win32/GDI, AppKit/CoreText, the browser DOM, and
  the terminal each use their platform's input and presentation APIs.
- **Build and release coverage.** CI checks the Rust workspace, checks Windows and macOS targets
  from Linux, exercises the template rename, and builds platform packages and browser assets.

## Start your own application

Use this repository as a starting point and follow the [bootstrap guide](doc/bootstrap.md). The
rename script handles mechanical project-wide changes; the guide walks through replacing the
example domain, keeping the core and CLI in sync, and adapting or removing shells.

```sh
scripts/bootstrap.py notes --name Notes --app-id io.github.you.Notes --repo https://github.com/you/notes
scripts/bootstrap.py --check quill
```

The bootstrap job in CI runs the rename and checks that the generated project builds and contains
no remaining `quill` references. That keeps the repository honest as a reusable template.

You do not have to take all of it. Smaller ways in:

- **Add a capability to the example** — a method on `App`, its CLI command, a `Command`, and one
  line per shell. The compiler and the tests name every place you missed
  ([the checklist](doc/architecture.md#7-checklists)).
- **Keep the core and CLI, pick your shells.** Dropping a shell is deleting a directory and the
  lines the tests then point at ([step 7](doc/bootstrap.md#7-dropping-a-shell-only-if-the-person-asked)).
- **Write a shell for another toolkit.** Start from the minimal shell, implement `Metrics` with the
  engine you draw with, and follow the "Adding a shell" checklist.
- **Take only the idea.** The ten rules in the architecture guide do not depend on Rust or on a
  text editor.

## Project layout

| Path | Role |
|---|---|
| `core/` | `quill-core`: document, history, layout, view, commands, and application API |
| `core/examples/minimal_shell.rs` | The shell contract in one file — start reading here |
| `cli/` | `quill`: scriptable interface and CLI parity checks |
| `ui_tui/` | `quill-tui`: terminal interface built with ratatui |
| `ui_gtk/` | `quill-gtk`: GNOME interface built with GTK 4 and libadwaita |
| `ui_web/` | `quill-web`: WebAssembly module and browser interface |
| `ui_win32/` | `quill-win32`: Windows interface built on Win32 and GDI |
| `ui_mac/` | `quill-mac`: macOS interface built on AppKit and CoreText |
| `examples/sample.sh` | Sample document assembled through CLI commands |
| `scripts/` | Local checks, shell launchers, and the bootstrap script |
| `doc/` | The tour, architecture, bootstrap guide, CI, shell conventions, and design decisions |

## Build and run

Install a current stable Rust toolchain. The core, CLI, terminal shell, browser module, and portable
parts of the Windows and macOS shells can be built on a typical Rust development machine:

```sh
cargo test
cargo run -p quill-cli -- --help
cargo run -p quill-tui -- notes.txt
scripts/check.sh
```

`scripts/check.sh` runs the checks available on the current machine. The GTK shell needs GTK
development packages. Browser builds need the `wasm32-unknown-unknown` target and the matching
`wasm-bindgen` CLI. Windows and macOS applications can be linked and run on their respective
platforms; their Rust code can also be type-checked for target platforms from Linux.

| Shell | Additional requirements | Example |
|---|---|---|
| GNOME | GTK 4 and libadwaita development packages | `cargo run -p quill-gtk -- notes.txt` |
| Browser | `wasm32-unknown-unknown` and matching `wasm-bindgen` CLI | `scripts/run.sh web` |
| Windows | Windows with MSVC build tools | `cargo run -p quill-win32 -- notes.txt` |
| macOS | Mac with Xcode command-line tools | `cargo run -p quill-mac -- notes.txt` |

For target checks and GUI screenshot workflows, see [doc/ci.md](doc/ci.md). CI also renders GUI
frames in headless jobs and checks that repeated renders are reproducible. Such checks validate
rendering and wiring; visual changes still need to be inspected.

## Use the CLI

The CLI makes application behaviour available to scripts and agents. Positions in reports are
1-based, and `--format json` provides structured output. Use `--session FILE` when state such as
caret position, selection, or undo history should persist between invocations.

```sh
quill new notes.txt
quill --session s.json type notes.txt --text 'hello world'
quill --session s.json undo notes.txt
quill view notes.txt --width 20
quill --session s.json move notes.txt down --width 20
quill --format json info notes.txt
quill commands
```

See [doc/cli-parity.md](doc/cli-parity.md) for the relationship between the core API and CLI.

## CI and current status

CI separates fast checks from release builds. It runs formatting, linting, documentation, tests,
licence checks, and the bootstrap check. Dedicated jobs check Windows and macOS code for their
target platforms; release jobs build Linux packages, a Windows executable, a macOS app and disk
image, and the browser bundle. See [doc/ci.md](doc/ci.md) for exactly what each job verifies.

Known limitations are documented in each shell's guide. The macOS shell is built and exercised by
CI, including headless frame rendering, but has not yet been used interactively on a Mac. The
current shells also do not support inline input-method composition, and GDI does not shape complex
scripts.

## License

MIT. See [LICENSE](LICENSE). The repository is REUSE-compliant.
