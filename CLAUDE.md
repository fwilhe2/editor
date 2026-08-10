# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Status

`core/`, `cli/` and `ui_tui/` are implemented and tested. `ui_linux/`, `ui_mac/` and `ui_windows/`
do not exist yet.

**MSRV is 1.85** (`rust-version` in the workspace manifest), matching the toolchain on the dev
machine. This actively constrains dependency choices: `ratatui` is pinned to 0.29 because 0.30 needs
1.88, and `Cargo.lock` holds `instability` and `darling` back for the same reason. If a build fails
with "rustc 1.85.0 is not supported by the following packages", pin the offending crate with
`cargo update -p <crate> --precise <older>` rather than raising the MSRV by accident.

Two things the spec calls for are deliberately **not** in the core yet, because nothing needs them:
UniFFI annotations and a `build.rs` (no foreign shell exists to consume bindings), and a tokio
runtime (no background work exists — every operation is synchronous and fast). `Editor` is already
shaped for UniFFI: an opaque object with interior mutability, `&self` methods, and plain scalars
across the boundary.

## Commands

```sh
cargo test --workspace          # 42 tests: core, CLI end-to-end, TUI render/key tests
cargo test -p editor-core       # one crate
cargo test undo                 # single test by name substring
cargo run -p editor-cli -- --help
cargo run -p editor-tui -- FILE
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

CI runs all of the above. `rustfmt` and `clippy` may not be installed locally (this machine has no
`rustup`); CI is the enforcement point.

Driving the TUI non-interactively, for when a change needs checking in a real terminal — always
under `timeout`, since `event::read()` blocks forever if the keys arrive before raw mode engages:

```sh
( sleep 2; printf 'text\r'; sleep 1; printf '\x13\x11' ) \
  | timeout 20 script -qec "target/debug/edit-tui FILE" /dev/null
```

## Scope

This is a **prototype of the architecture concept, not a competitive editor**. The feature set is
deliberately minimal — it exists to prove that one Rust core can drive five very different
front-ends. When in doubt, do not add features; add them to the core only if every shell (including
the CLI) can expose them. Breadth across platforms is the deliverable; depth of editing features is
explicitly not.

## Target architecture: Shared Core, Native Shell

All editor logic, state, and I/O live in one pure-Rust crate (`core/`). Every UI is a "dumb"
renderer and event forwarder — it holds no editor state of its own. Two classes of shell consume
the core differently, and this split is the main thing to keep straight:

- **Rust shells** (`cli/`, `ui_tui/`, `ui_linux/`) depend on `core` as an ordinary Cargo dependency
  and call its public API directly. No FFI, no bindings, no translation layer.
- **Foreign shells** (`ui_mac/`, `ui_windows/`) reach the core through UniFFI-generated bindings.
  macOS builds the core as an `.xcframework` and consumes generated Swift; Windows builds a `.dll`
  and consumes generated C# (via `uniffi-bindgen-cs`). A `build.rs` in `core/` generates the Swift
  and C# scaffolding during `cargo build`.

Layout (Cargo workspace at the root; ✅ exists, ⬜ planned):

```
core/         ✅ editor-core  — Rust logic, state, undo history
cli/          ✅ editor-cli   — the `edit` binary
ui_tui/       ✅ editor-tui   — the `edit-tui` binary (ratatui)
ui_linux/     ⬜ gtk4-rs UI (pure Rust)
ui_mac/       ⬜ Xcode project, SwiftUI/AppKit + generated Swift bindings
ui_windows/   ⬜ Visual Studio project, C#/WinUI 3 + generated C# bindings
```

## Core design constraints

These are decisions from the spec that are expensive to reverse later:

- **Text is a Rope** (`ropey` or `crop`), never `String`/`Vec<String>`. Insertions and deletions must
  stay O(log N) for large files.
- **`get_viewport(start_line, end_line)`** is the only way a shell reads text. Never expose an API
  that hands the whole buffer to the UI — that defeats the point of the rope.
- **Undo/redo is a command pattern inside the core**: an `Action` enum (`Insert`, `Delete`, …) with
  `undo_stack`/`redo_stack` owned by the core. Shells never implement undo themselves.
- **The core owns its async runtime** (tokio on background threads). Core API calls invoked from a
  UI thread must return immediately; long work is dispatched to the runtime.
- **State changes flow back via foreign traits** — interfaces declared in Rust and implemented by
  Swift/C#/Rust shells. The core calls into them to trigger redraws. Shells must not poll.
- `EditorState` is wrapped in a `RwLock`/`Mutex` and exported as a UniFFI Object; the public API is
  annotated with `#[uniffi::export]`.

## Core API surface

Shells hold an `Arc<Editor>` (`core/src/lib.rs`) and call:

- documents — `open`, `load_file`, `save_file`, `save_file_as`
- editing — `handle_input(char)`, `insert_text(&str)`, `handle_backspace()`, `undo()`, `redo()`
- cursor/view — `move_cursor(Direction)`, `set_cursor(Position)`, `cursor()`,
  `get_viewport(start, end)`, `scroll_offset()` / `set_scroll_offset()`
- inspection — `line_count`, `char_count`, `is_dirty`, `path`, `can_undo`, `can_redo`
- notification — `set_observer(Arc<dyn EditorObserver>)`
- persistence for stateless callers — `session()` / `restore_session(Session)`

Every method takes `&self`; the `RwLock` lives inside `Editor`, so one editor can be shared across a
UI thread and background work. `EditorState` (`core/src/state.rs`) is crate-private — shells cannot
reach the rope directly, only through `get_viewport`.

Shells map native events onto these: arrow keys → `move_cursor`, character keys → `handle_input`,
Ctrl+S → `save_file`, Ctrl+Q → shell teardown.

Non-obvious invariants in the implementation:

- **`Editor::mutate` drops the write lock before notifying observers.** An observer is expected to
  call back into the editor to re-read state; notifying while holding the lock deadlocks. There is a
  test for this (`an_observer_may_read_the_editor_without_deadlocking`).
- **Positions in the core are 0-based**; only the CLI is 1-based (see below).
- **`get_viewport`'s `end_line` is exclusive**, and a `start_line` past the end clamps to the last
  line rather than returning nothing — a stale scroll offset must never blank the view.
- **Undo/redo cursor placement falls out of `Action::inverse`**, not from special-casing: undoing an
  `Insert` applies a `Delete` and lands the cursor at `at`.
- Line terminators are assumed to be LF. Backspace removes a single `char`, so a CRLF file loses the
  `\n` and keeps a stray `\r`. Acceptable for the prototype; fix in `EditorState::backspace` if it
  ever matters.

**Feature parity is a hard rule.** Anything reachable from any GUI or the TUI must also be reachable
from `cli/`. In practice this means no capability may live in a shell — if a UI needs a behavior the
core doesn't have, the behavior goes into the core and the CLI gains a subcommand for it in the same
change. A UI-only feature is a bug.

## The CLI (`cli/`, binary `edit`)

A non-interactive, scriptable front-end over the same core API — and the thing that keeps the parity
rule honest. Three audiences at once: agents, humans in a shell, and scripts/CI. No prompts, no TTY
assumptions; stdout is parseable, diagnostics go to stderr, failures exit non-zero.

Subcommands: `new`, `view`, `insert`, `backspace`, `move`, `undo`, `redo`, `info`. Global flags:
`--session`, `--format text|json`, `--dry-run`.

Three decisions to keep in mind before changing it:

- **The CLI is 1-based, the core is 0-based.** Column 1 is before the first character. `report::Cursor`
  (`to_core` / `from_core`) is the *only* place the two meet — never convert anywhere else.
- **A stateless process needs somewhere to keep state.** Each invocation loads, applies one command,
  and writes back. Cursor position and undo history survive only via `--session <file>`, which
  serializes `core::Session`. Consequently `undo`/`redo` without `--session` is an error, not a
  silent no-op — the stacks would always be empty.
- **`--text` accepts hyphen-leading values** (`allow_hyphen_values`), because inserting arbitrary
  text is the point; a bare `-` reads stdin instead.

`text` output is deliberately bare — `view` prints just the lines, so it pipes into `grep`/`wc`.
`json` emits a single object; `changed` distinguishes a real edit from a no-op, `written` says
whether disk was touched.

## Platform conventions

Each native shell must follow its own platform's guidelines rather than a shared house style. A UI
that looks the same on all three is the wrong outcome.

- **Linux (`ui_linux/`)** — GNOME Human Interface Guidelines: libadwaita patterns, header bars,
  GNOME keyboard conventions, adaptive layout.
- **macOS (`ui_mac/`)** — Apple Human Interface Guidelines: the standard menu bar, macOS keyboard
  shortcuts (⌘S, ⌘Q), native window/toolbar behavior.
- **Windows (`ui_windows/`)** — Microsoft's WinUI 3 / Fluent design docs: Fluent controls, Mica
  backdrop, Windows keyboard conventions.

Consult the current published guidelines when building UI; do not copy conventions from one shell to
another.

## CI

Every shell gets its own GitHub Actions workflow that builds it. `core-cli.yml` covers the core and
the CLI (tests on all three OS runners, plus a single fmt/clippy job for the whole workspace);
`tui.yml` covers `ui_tui` the same way.

**Cross-compilation is not an option** — each app builds on its own OS runner. macOS builds on
`macos-*` (Xcode), Windows on `windows-*` (MSBuild / Windows App SDK), Linux and the pure-Rust
shells on `ubuntu-*` (GTK4 dev packages required for `ui_linux`). Never add a workflow that attempts
to produce a macOS or Windows app from a Linux runner. The foreign-shell workflows must build the
Rust core first (`.xcframework` on macOS, `.dll` on Windows) and generate bindings before the native
project step.

## The TUI (`ui_tui/`, binary `edit-tui`)

`ratatui` for layout and rendering, `crossterm` for raw mode and input — not raw ncurses.
**crossterm is used via `ratatui::crossterm`**, never as a direct dependency, so the two versions
cannot drift apart. `main.rs` owns the terminal lifecycle; `app.rs` owns event routing and drawing
and holds no editor state — every frame is derived from the core.

- **Teardown runs on every exit path.** Normal quit, error return, and panic (via a hook installed
  in `setup_terminal`). A missed teardown leaves the user's shell in raw mode with no echo. `run()`
  restores *before* propagating an error, so the message isn't printed into a raw terminal.
- **Only `KeyEventKind::Press` is acted on.** Windows also reports Release; handling both types
  every character twice.
- **Redraws are driven by `RedrawFlag`,** which is both the core's `EditorObserver` and the shell's
  own signal. The core raises it on document changes; `on_key` raises it for status-line changes the
  core knows nothing about (a failed undo, the quit prompt). The loop blocks on `event::read()` and
  only draws when the flag is set.
- **Vertical scrolling goes through the core's `scroll_offset`** (`follow_cursor`), so every shell
  scrolls by the same rule.
- **There is no horizontal scrolling.** On lines wider than the terminal the caret parks at the
  right edge. Adding it means deciding whether a horizontal offset belongs in the core next to
  `scroll_offset` — and if it does, the CLI needs the matching capability.

Ctrl+Q on a dirty document warns once and quits on a second press; any other key cancels.

The `TestBackend` tests in `app.rs` render into an off-screen buffer and assert on the text, which
covers key routing, scrolling and the status bar without a terminal.
