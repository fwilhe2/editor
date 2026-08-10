# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Status

Every shell in the original plan exists: `core/`, `cli/`, `ffi/`, `ui_tui/`, `ui_linux/`,
`ui_windows/` and `ui_mac/`. Only the planned `ui_qt/` (see below) is outstanding.

**MSRV is 1.85** (`rust-version` in the workspace manifest), matching the toolchain on the dev
machine. This actively constrains dependency choices: `ratatui` is pinned to 0.29 because 0.30 needs
1.88, and `Cargo.lock` holds `instability` and `darling` back for the same reason. If a build fails
with "rustc 1.85.0 is not supported by the following packages", pin the offending crate with
`cargo update -p <crate> --precise <older>` rather than raising the MSRV by accident.

The core still has **no tokio runtime**, because nothing needs one — every operation is synchronous
and fast. Add it when the core gains work that must not block a UI thread.

## Commands

```sh
cargo test --workspace          # 58 tests; needs libgtk-4-dev + libadwaita-1-dev for ui_linux
cargo test -p editor-core       # one crate
cargo test undo                 # single test by name substring
cargo run -p editor-cli -- --help
cargo run -p editor-tui -- FILE
cargo run -p editor-gtk -- FILE
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

`--workspace` only works on Linux with the GTK development packages installed; CI builds `ui_linux`
in its own Linux-only job and tests the other crates by name. `rustfmt` and `clippy` may not be
installed locally (this machine has no `rustup`); CI is the enforcement point.

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

- **Rust shells** (`cli/`, `ui_tui/`, `ui_linux/`, and `ui_qt/` if it uses `cxx-qt`) depend on
  `core` as an ordinary Cargo dependency and call its public API directly. No FFI, no bindings, no
  translation layer.
- **Foreign shells** (`ui_windows/`, and `ui_mac/` when it exists) reach the core through
  UniFFI-generated bindings produced from the **`ffi/` crate**, not from `core` directly. Windows
  builds `editor_ffi.dll` and consumes generated C#; macOS will build an `.xcframework` and consume
  generated Swift.

Layout (Cargo workspace at the root; ✅ exists, ⬜ planned):

```
core/         ✅ editor-core  — Rust logic, state, undo history
cli/          ✅ editor-cli   — the `edit` binary
ffi/          ✅ editor-ffi   — UniFFI facade + a C# smoke test of the boundary
ui_tui/       ✅ editor-tui   — the `edit-tui` binary (ratatui)
ui_linux/     ✅ editor-gtk   — the `edit-gtk` binary (GTK4 + libadwaita)
ui_windows/   ✅ EditorApp    — C# / WinUI 3, consuming generated bindings
ui_mac/       ✅ EditorApp    — SwiftUI (SwiftPM package), generated Swift bindings
ui_qt/        ⬜ Qt shell (see "Planned: the Qt shell")
```

`ui_linux/` is GTK/GNOME-specific despite the name. Once `ui_qt/` exists — Qt runs on all three
platforms — renaming it to `ui_gtk/` would be more honest. The spec's name is kept for now.

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
  `get_viewport(start, end)`, `scroll_offset()` / `set_scroll_offset()`, `follow_cursor(height)`
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
- **`follow_cursor` deliberately does not notify when the offset does not move.** Shells call it
  while laying out a frame; an unconditional notification would have every repaint request the next
  one, forever. There is a test pinning this.
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

Every shell gets its own GitHub Actions workflow that builds it: `core-cli.yml` (three OS runners
plus one workspace-wide fmt/clippy job), `tui.yml` (three OS runners), `linux.yml` (Ubuntu only),
`windows.yml` and `macos.yml` (Windows/macOS only — Rust library, then bindings, then the FFI smoke
test, then the app; the smoke test running before the UI build is what separates a binding failure
from a XAML/SwiftUI one).

Two traps when adding a shell with system dependencies: the shared jobs must stop using
`--workspace` where the new crate cannot build (the core/CLI test job names its crates for exactly
this reason), and the fmt/clippy job *does* lint the whole workspace, so it needs those system
packages installed even though it produces no binaries.

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

## The GTK shell (`ui_linux/`, binary `edit-gtk`)

GTK4 through libadwaita, following the GNOME HIG: `AdwHeaderBar`, `AdwAlertDialog` for unsaved
changes on close, `AdwToastOverlay` for save feedback. **gtk is reached through `libadwaita::gtk`**,
never as a direct dependency, so the versions cannot drift.

- **The `GtkTextView` is a renderer, not the document.** It is `editable(false)` with
  `cursor_visible(true)`, and the key controller runs in the **Capture** phase so GTK never sees a
  key we handle. Every repaint refills the buffer from `get_viewport` and places the caret from the
  core's cursor. This is the single most important thing to preserve: the moment the TextView is
  allowed to edit itself, there are two sources of truth.
- **Refreshes come from the core's observer.** `Notifier` forwards `state_changed` over an
  `async_channel` that a `spawn_future_local` task drains, because `EditorObserver` is `Send + Sync`
  and GTK widgets are neither. The drain loop coalesces bursts, so one keystroke is one repaint.
- **`refresh()` renders from `scroll_offset` and never calls `follow_cursor`.** Key handling calls
  `follow_cursor` explicitly. If `refresh` did it, dragging the scrollbar away from the cursor would
  snap straight back.
- **`syncing: Cell<bool>`** suppresses the adjustment's `value-changed` while `refresh` is writing
  to it, so the widget and the core do not chase each other.
- `visible_lines()` derives the viewport height from Pango metrics and the allocated height. Before
  the first allocation that is 0, hence the one-line floor and the repaint on `default-height`.

`keymap.rs` is deliberately widget-free — key/modifier in, `UiAction` out — which is why it can be
unit-tested with no display, and it is the pattern the Qt shell should copy.

## The FFI layer (`ffi/`) and the Windows shell (`ui_windows/`)

**The UniFFI annotations live in `ffi/`, not in `core/`** — a deliberate departure from the original
spec. `Editor` takes `impl AsRef<Path>` and returns `PathBuf`, `char` and `Option<PathBuf>`, none of
which cross an FFI boundary; exporting it directly would mean degrading the Rust API to Strings and
non-generic signatures for the benefit of foreign callers. `EditorHandle` in `ffi/` is a thin facade
— every method forwards to exactly one core call, so there is nowhere for behaviour to drift — and
the Rust shells never compile UniFFI at all.

Versions are coupled and must be bumped together: **`uniffi` in `Cargo.toml` and the
`uniffi-bindgen-cs` tag in `.github/workflows/windows.yml`** (currently 0.31 / `v0.11.0+v0.31.0`).
The generator lags upstream uniffi, so uniffi's latest release is usually *not* the one to use.
Swift has no such problem — its generator is built from `ffi/` itself.

Regenerating bindings by hand:

```sh
cargo build --release -p editor-ffi
cargo install uniffi-bindgen-cs --git https://github.com/NordSecurity/uniffi-bindgen-cs \
  --tag v0.11.0+v0.31.0
uniffi-bindgen-cs --library target/release/libeditor_ffi.so --out-dir ui_windows/Generated
```

- **The generated types are `internal`.** They must be compiled *into* the consuming assembly; a
  project reference will not see them. Both `ui_windows/` and `ffi/csharp-smoke/` include the
  generated `.cs` as a source file, and `ui_windows/Generated/` is gitignored.
- **`ffi/csharp-smoke/` is where the boundary is actually tested.** It is a console app, so it runs
  on Linux against `libeditor_ffi.so` exactly as it runs on Windows against `editor_ffi.dll` — which
  makes the FFI layer verifiable without a Windows machine. If it passes and the WinUI app
  misbehaves, the bug is in XAML, not the bindings. Run it locally with
  `LD_LIBRARY_PATH=target/release dotnet run --project ffi/csharp-smoke`.
- **Positions stay 0-based across the boundary.** Each shell adds one for display.
- The WinUI `TextBox` is read-only and rendered from `Viewport`, for the same reason the GTK
  `TextView` is: letting the control edit itself would create a second source of truth. `KeyDown`
  handles navigation and editing keys, `CharacterReceived` handles text (skipped while Ctrl is
  down, or Ctrl+S would type a control character), and shortcuts are `KeyboardAccelerator`s.
- The app is **unpackaged** (`WindowsPackageType=None`) so CI can build it without signing
  certificates. `editor_ffi.dll` is copied next to the executable by the csproj.
- Known gap: no mouse-wheel scrolling — the viewport moves only via `follow_cursor`.

## The macOS shell (`ui_mac/`)

SwiftUI over the same `ffi/` crate. **Swift bindings come from uniffi itself**, via a
`uniffi-bindgen` binary inside `ffi/`, so the generator is always on the same uniffi version as the
runtime — unlike C#, whose external generator must be matched by hand.

```sh
cargo build --release -p editor-ffi
./ui_mac/generate-bindings.sh release
swift build --package-path ui_mac --product FfiSmoke -Xlinker "$PWD/target/release/libeditor_ffi.a"
./ui_mac/.build/debug/FfiSmoke
```

- **SwiftPM, not a `.xcodeproj`.** A hand-written `.pbxproj` is unreviewable and easy to corrupt;
  Xcode opens the package directly and `xcodebuild` builds it. CI assembles `Editor.app` around the
  SwiftPM executable with `ui_mac/Info.plist` — without that plist the process gets no menu bar.
- **`swift-tools-version: 5.9` on purpose.** Swift 6's strict concurrency rejects the generated
  bindings' `@unchecked Sendable`.
- **The module map in `Sources/EditorFFI/include/` is hand-written and committed.** The one uniffi
  emits declares `use "Darwin"` and only works on Apple platforms; ours also builds on Linux, which
  is what lets `FfiSmoke` run without a Mac. `generate-bindings.sh` deliberately does not copy it.
- **The app links `libeditor_ffi.a`, not the dylib**, so the bundle has no library to find at
  runtime. That is why `ffi/` builds `staticlib` as well as `cdylib`.
- **`FfiSmoke` mirrors `ffi/csharp-smoke`** and is the same 13 checks. Both run on Linux, so both
  FFI boundaries are verifiable here; what is *not* verifiable locally is SwiftUI and XAML.
- Named constructors become static methods, not initialisers: `EditorHandle.open(path:)`, not
  `EditorHandle(path:)`. Only a constructor called `new` maps to `init`.
- Keys go through `.onKeyPress` (macOS 14+); ⌘-shortcuts are returned as `.ignored` so the menu bar
  handles them. `CommandGroup(replacing: .undoRedo)` is what stops AppKit's own undo stack — which
  knows nothing about the document — from taking ⌘Z.
- Known gap: no scrolling by mouse or trackpad; the viewport follows the caret only.

## Planned: the browser shell (`ui_web/`)

Not written. The core is pure Rust with no platform assumptions, so it already compiles to
`wasm32-unknown-unknown`; a web shell would reach it through **`wasm-bindgen`, not UniFFI**, making a
third class of shell alongside "Rust direct" and "UniFFI foreign". `get_viewport` is the part worth
proving there — a DOM renderer is the furthest thing from the rope, and if the viewport API survives
it, the boundary is right. The web has its own conventions; treat it as a platform to respect, not
as the excuse to stop respecting any.

## Planned: the Qt shell (`ui_qt/`)

Not written yet. Two things have already been done in anticipation of it: `follow_cursor` was moved
out of the TUI into the core (three shells must not each invent a scroll rule), and the GTK shell
keeps its key mapping in a widget-free, testable module.

**Route to prefer: `cxx-qt`.** It puts the Qt object model on the Rust side, so `ui_qt/` stays a
plain Cargo crate depending on `core` directly — no FFI, the same class of shell as the TUI and GTK
ones. The alternative, a C++ Qt application calling into the core, needs a hand-rolled C ABI or a
`cxx` bridge: **UniFFI has no C++ target**, so that route adds a third binding mechanism alongside
"Rust direct" and "UniFFI foreign". Only take it if a C++ codebase is a requirement.

When it is built:

- Qt is cross-platform, so it gets built on all three OS runners like the TUI, never cross-compiled.
  Qt itself comes from `jurplel/install-qt-action` or the distro packages.
- `QPlainTextEdit`/`QTextDocument` has exactly the GTK problem — it owns a document. Make it
  read-only and drive it from `get_viewport`, as `ui_linux/` does.
- Follow the KDE HIG on Plasma. Do not copy the GNOME shortcut set wholesale; they agree on
  Ctrl+S/Z/Shift+Z, but the window furniture differs.
- Parity still applies: nothing may appear in the Qt shell that the CLI cannot do.
