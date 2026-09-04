# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

For the pattern itself — how to build an app this way from scratch, rather than how this one is put
together — see [`doc/shared-core-native-shell.md`](doc/shared-core-native-shell.md).

## Status

Every shell in the original plan exists: `core/`, `cli/`, `ffi/`, `ui_tui/`, `ui_linux/`,
`ui_win32/` and `ui_mac/`, plus `ui_web/` and `ui_egui/`, both planned later. **`ui_qt/` (see below)
is the only outstanding one.**

`ui_egui/` is complete — all seven stages of [`doc/plan-egui-shell.md`](doc/plan-egui-shell.md) have
landed, including its workflow and the documentation. Its invariants have their own section below,
like every other shell's. It cost the core nothing: no new capability, hence no new CLI subcommand,
which is what the parity rule predicts for a shell that adds a toolkit rather than a platform
capability.

**`ui_windows/` no longer exists.** The WinUI 3 application in C# was replaced by `ui_win32/`, a
Rust-direct shell drawing a plain Win32 window with GDI, in
[`doc/decision-win32-shell.md`](doc/decision-win32-shell.md). `ffi/csharp-smoke/` went with it, and
so did the `uniffi-bindgen-cs` pin. Two things follow that matter more than the shell itself:

- **`ffi/` is now exercised by one language, not two.** Swift is the only foreign binding left. The
  facade crate stays exactly as it was, and `FfiSmoke` still checks the boundary on Linux — but the
  architecture's "bindings into any language" claim has half the evidence it did. This is the
  decision's stated price, not an oversight.
- **The Windows shell can now be type-checked from Linux**, which no native shell here could before:
  `rustup target add x86_64-pc-windows-msvc` and `cargo check -p editor-win32 --target
  x86_64-pc-windows-msvc`. `cargo check` never links, so no MSVC is needed. Use it before pushing
  anything that touches `ui_win32/`.

**There is no MSRV.** `rust-version` was removed from the workspace manifest, and the pins that
served it are gone with it: `ratatui` is on 0.30, `instability` and `darling` are unpinned. The
policy is "builds on current stable", which is what CI actually tests — the old 1.85 floor was
enforced by accident, because the dev machine had nothing newer, and once rustup arrived nothing
checked it at all. A stated-but-unchecked minimum is worse than none.

So: take dependency updates freely. If one ever needs to be held back it should be for a reason that
is written down at the pin, not for a compiler version nobody verifies.

The core still has **no tokio runtime**, because nothing needs one — every operation is synchronous
and fast. Add it when the core gains work that must not block a UI thread.

## Commands

```sh
cargo test --workspace          # 140 tests; needs libgtk-4-dev + libadwaita-1-dev for ui_linux
cargo test -p editor-core       # one crate
cargo test undo                 # single test by name substring
cargo run -p editor-cli -- --help
cargo run -p editor-tui -- FILE
cargo run -p editor-gtk -- FILE
cargo run -p editor-egui -- FILE    # no system dependencies, any platform
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings

# The Windows shell, checked without Windows. `cargo check` does not link, so this
# needs no MSVC — only the target's standard library. Run it before pushing any
# change to ui_win32/; the alternative is finding out from the Windows runner.
rustup target add x86_64-pc-windows-msvc
cargo check -p editor-win32 --target x86_64-pc-windows-msvc
cargo clippy -p editor-win32 --target x86_64-pc-windows-msvc --all-targets -- -D warnings

# And it can actually be run here. cargo-xwin links the real MSVC binary on Linux
# (clang + lld + Microsoft's SDK), Wine runs it, Xvfb gives it a display, and
# ImageMagick photographs it. Installed on this machine already.
cargo xwin build -p editor-win32 --release --target x86_64-pc-windows-msvc
Xvfb :99 -screen 0 1200x800x24 &
export DISPLAY=:99 WINEDLLOVERRIDES="mscoree,mshtml=" WINEDEBUG=-all
wine target/x86_64-pc-windows-msvc/release/edit-win32.exe FILE &
import -window root /tmp/shot.png

./ui_web/build.sh release     # wasm module + JS glue + static files -> ui_web/dist
./ui_web/smoke.sh release     # drive the built module in jsdom, no browser needed
python3 -m http.server --directory ui_web/dist 8000
```

`--workspace` only works on Linux with the GTK development packages installed; CI builds `ui_linux`
in its own Linux-only job and tests the other crates by name.

Every command above runs on this machine as written: rustup is installed, with `rustfmt`, `clippy`,
the `wasm32-unknown-unknown` target and a matching `wasm-bindgen` CLI. Run fmt and clippy before
pushing rather than discovering them in CI — noting that the default toolchain here is *nightly*, so
a lint that fires locally may not exist on the stable CI uses, and vice versa.

`editor-web` is in the workspace and builds and tests on the host like any other crate — `web-sys`
compiles anywhere, it just cannot run. Only `ui_web/build.sh` needs the wasm target and the
`wasm-bindgen` CLI, and on a machine without them it says so and stops.

Driving the TUI non-interactively, for when a change needs checking in a real terminal:

```sh
cargo build -p editor-tui
./ui_tui/drive.py FILE 'hi\r' '\x13' '\x11'    # type "hi" + Enter, Ctrl+S, Ctrl+Q
```

Each argument is one burst of input, sent a beat apart so raw mode is in place first. **Piping into
`script` no longer works**: since ratatui 0.30 the terminal is asked where the cursor is (`ESC[6n`)
during startup and start-up blocks until something answers, which a pipe never does — the binary
gives up with "The cursor position could not be read within a normal duration". `drive.py` opens a
real pty and plays terminal, reply included. It strips escape sequences from what it prints, so the
text is readable but the layout is not; for layout, look at it yourself.

## Scope

This is a **prototype of the architecture concept, not a competitive editor**. The feature set is
deliberately minimal — it exists to prove that one Rust core can drive seven very different
front-ends. When in doubt, do not add features; add them to the core only if every shell (including
the CLI) can expose them. Breadth across platforms is the deliverable; depth of editing features is
explicitly not.

## Target architecture: Shared Core, Native Shell

All editor logic, state, and I/O live in one pure-Rust crate (`core/`). Every UI is a "dumb"
renderer and event forwarder — it holds no editor state of its own. Three classes of shell consume
the core differently, and this split is the main thing to keep straight:

- **Rust shells** (`cli/`, `ui_tui/`, `ui_linux/`, `ui_win32/`, and `ui_qt/` if it uses `cxx-qt`)
  depend on `core` as an ordinary Cargo dependency and call its public API directly. No FFI, no
  bindings, no translation layer.
- **Foreign shells** — `ui_mac/`, and only `ui_mac/` since the C# shell was replaced — reach the core
  through UniFFI-generated bindings produced from the **`ffi/` crate**, not from `core` directly.
  macOS builds a static library and consumes generated Swift. The facade is still written to serve
  more than one language; it just has one caller now.
- **The browser shell** (`ui_web/`) is both at once: Rust depending on `core` directly, compiled to
  `wasm32-unknown-unknown`, reaching its platform through **`wasm-bindgen`**. UniFFI has no
  JavaScript target, and would be pointless when the shell is Rust anyway — so `ffi/` is not
  involved at all.

Layout (Cargo workspace at the root; ✅ exists, ⬜ planned):

```
core/         ✅ editor-core  — Rust logic, state, undo history
cli/          ✅ editor-cli   — the `edit` binary
ffi/          ✅ editor-ffi   — UniFFI facade for the Swift shell
ui_tui/       ✅ editor-tui   — the `edit-tui` binary (ratatui)
ui_linux/     ✅ editor-gtk   — the `edit-gtk` binary (GTK4 + libadwaita)
ui_win32/     ✅ editor-win32 — the `edit-win32` binary (Win32 + GDI), no runtime deps
ui_mac/       ✅ EditorApp    — SwiftUI (SwiftPM package), generated Swift bindings
ui_web/       ✅ editor-web   — wasm32 + wasm-bindgen, rendered into the DOM
ui_egui/      ✅ editor-egui  — the `edit-egui` binary (eframe), portable, native to nothing
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

- documents — `open`, `load_file`, `save_file`, `save_file_as`, and for shells with no filesystem,
  `load_text(name, text)` / `save_to_string(name)`
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
- **`load_text` / `save_to_string` are the file API for platforms the core cannot read or write on
  its own** — the browser, where a file arrives as a string from the File API and leaves as a
  download. `load_text` is a *load*, not a large insert: it clears the history, because undoing into
  a document that was replaced would resurrect text the file never had. `save_to_string` is the one
  place a shell legitimately receives the whole buffer; rendering still goes through `get_viewport`.
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

Subcommands: `new`, `view`, `export`, `import`, `insert`, `backspace`, `move`, `undo`, `redo`,
`info`. Global flags: `--session`, `--format text|json`, `--dry-run`.

Four decisions to keep in mind before changing it:

- **The CLI is 1-based, the core is 0-based.** Column 1 is before the first character. `report::Cursor`
  (`to_core` / `from_core`) is the *only* place the two meet — never convert anywhere else.
- **A stateless process needs somewhere to keep state.** Each invocation loads, applies one command,
  and writes back. Cursor position and undo history survive only via `--session <file>`, which
  serializes `core::Session`. Consequently `undo`/`redo` without `--session` is an error, not a
  silent no-op — the stacks would always be empty.
- **`--text` accepts hyphen-leading values** (`allow_hyphen_values`), because inserting arbitrary
  text is the point; a bare `-` reads stdin instead.
- **`export`/`import` exist because the browser shell does.** They are the CLI's half of
  `save_to_string`/`load_text`: `export` writes the document to stdout byte for byte (unlike `view`,
  which prints a range of lines and normalises the ends), `import` replaces it wholesale and drops
  the undo history with it. Added in the same change as `ui_web/`, which is what the parity rule
  demands.

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
- **Windows (`ui_win32/`)** — Windows' *conventions* without Windows' *controls*: the shell font
  from `SPI_GETNONCLIENTMETRICS`, the user's `SPI_GETWHEELSCROLLLINES`, the system caret, Ctrl+Y for
  redo, a Save/Don’t Save/Cancel dialog on close, a dark title bar via
  `DWMWA_USE_IMMERSIVE_DARK_MODE`, per-monitor DPI v2. There are no Fluent controls and no Mica —
  see the decision record for why that trade was taken.
- **The browser (`ui_web/`)** — the web's own conventions, which are as real as any desktop's:
  system font stack, `prefers-color-scheme` rather than a chosen theme, visible focus rings,
  Ctrl-*and*-⌘ shortcuts, files through the File API and a download, `beforeunload` in place of a
  close dialog. It is a platform to respect, not the place where respecting platforms stops.

Consult the current published guidelines when building UI; do not copy conventions from one shell to
another.

## CI

Every shell gets its own GitHub Actions workflow that builds it: `core-cli.yml` (three OS runners
plus one workspace-wide fmt/clippy job), `tui.yml` and `egui.yml` (three OS runners each, no system
packages), `linux.yml` (Ubuntu only), `macos.yml` (macOS only — Rust library, then bindings, then
the FFI smoke test, then the app; the smoke test running before the UI build is what separates a
binding failure from a SwiftUI one), `win32.yml` (see below), and `web.yml` (Ubuntu only, because
the browser is not an operating system: wasm is the same artifact everywhere).

The Win32 shell can also be **run** on Linux — `cargo-xwin` links the genuine MSVC binary and Wine
executes it under Xvfb; the recipe is in Commands above and in the decision record. Treat it exactly
as `cargo check --target` is treated: an inspection aid, never a build path, and **never in CI**. A
green Wine run says nothing about Windows' compositor, its DWM attributes or its fonts, and adding
it to a workflow would turn a debugging convenience into a release path. Rule 9 is unchanged.

`win32.yml` has **two** jobs, and the second is the unusual one. `build` is an ordinary
`windows-latest` job — test, clippy, build, then read the executable's import table back and fail if
anything outside Windows appears, which is what keeps the shell's central claim honest.
`check-from-linux` runs `cargo check -p editor-win32 --target x86_64-pc-windows-msvc` on Ubuntu:
`cargo check` never links, so it needs no MSVC, and it catches a windows-rs API break in about a
minute on the cheapest runner. **That is not cross-compilation and must not become it** — the
`.exe` still comes off the Windows runner. No native shell here could be inspected this way before.

`egui.yml` is the only one whose test step checks a GUI's *behaviour* rather than compiling it, and
it deliberately sets up no display — a run that needed one would mean the harness had stopped being
headless. Being portable is not a reason to build it once: it still gets three runners, because
what a portable toolkit centralises is verification, not distribution. The Ubuntu job alone is the
complete check; the other two are for artifacts and platform surprises.

Adding `ui_egui` also made the shared **lint** job compile eframe on every push, since it does say
`--workspace`. That cost is expected and `Swatinem/rust-cache` absorbs it.

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

## The FFI layer (`ffi/`)

**The UniFFI annotations live in `ffi/`, not in `core/`** — a deliberate departure from the original
spec. `Editor` takes `impl AsRef<Path>` and returns `PathBuf`, `char` and `Option<PathBuf>`, none of
which cross an FFI boundary; exporting it directly would mean degrading the Rust API to Strings and
non-generic signatures for the benefit of foreign callers. `EditorHandle` in `ffi/` is a thin facade
— every method forwards to exactly one core call, so there is nowhere for behaviour to drift — and
the Rust shells never compile UniFFI at all.

**`ui_mac/` is now its only consumer.** The C# shell that used to be the other one was replaced by
`ui_win32/`, which depends on `core` directly; see
[`doc/decision-win32-shell.md`](doc/decision-win32-shell.md). Two consequences:

- **The `uniffi` ↔ `uniffi-bindgen-cs` version coupling is gone**, and with it the worst pin in the
  repository — a hand-matched tag on a third-party generator that lagged upstream uniffi. `uniffi`
  in `Cargo.toml` can now be bumped on its own. Swift's generator is built from `ffi/` itself and
  has always been on the right version by construction.
- **Keep the facade language-neutral anyway.** It is written to serve any UniFFI target and the
  point of it is that a second language could be added without touching `core`. Do not let
  Swift-shaped assumptions leak into it just because Swift is the only caller today.

`ffi/csharp-smoke/` is gone with the C# shell. `FfiSmoke` in `ui_mac/` is the surviving boundary
check, it covers the same 13 assertions, and it still runs on Linux — so the FFI boundary remains
verifiable on this machine, by one harness instead of two.

- **Positions stay 0-based across the boundary.** Each shell adds one for display.
- `ffi/` still builds both `cdylib` and `staticlib`: the macOS app links the static library so the
  bundle has nothing to find at runtime, and `ui_mac/generate-bindings.sh` reads the `.so` on Linux.

## The Win32 shell (`ui_win32/`, binary `edit-win32`)

A plain Win32 window drawn with GDI through Microsoft's `windows` crate — Rust-direct, like the CLI,
TUI, GTK and egui shells. It replaced a WinUI 3 application in C#; the reasoning, the options
weighed and the costs accepted are in
[`doc/decision-win32-shell.md`](doc/decision-win32-shell.md).

**The point of it is one sentence: the `.exe` depends on nothing Windows does not ship.** No .NET
runtime, no Windows App SDK, no Visual C++ redistributable. Everything below serves that or follows
from it.

- **`.cargo/config.toml` links the MSVC CRT statically** (`-C target-feature=+crt-static`) for both
  Windows targets. Without it the binary needs `vcruntime140.dll`, which is *not* part of Windows —
  `ucrtbase.dll` is, and that is the distinction that makes this load-bearing rather than a
  preference. `win32.yml` reads the import table out of the built executable and fails on anything
  outside the OS, so the claim is checked on every push.
- **`keymap.rs` and `layout.rs` contain no Windows types at all** — a virtual-key code is a `u16`,
  a modifier is a `bool`, a pixel is an `i32`. That is what lets `cargo test -p editor-win32` run
  their 23 assertions on Linux. The `windows` dependency is declared under
  `[target.'cfg(windows)'.dependencies]`, and `main.rs` carries `#[cfg_attr(not(windows),
  allow(dead_code))]` on both modules so the host build stays clippy-clean.
- **The VK constants are copied from `winuser.h`, and a `#[cfg(windows)]` test pins every one of
  them against the real `windows` crate values.** Copying a wrong number is the obvious failure mode
  of doing it that way, so it is the first thing the Windows job checks.
- **No `EDIT` or rich-edit control, ever.** They own their own text buffer. Same rule that keeps the
  `GtkTextView` read-only and `contenteditable` out of `ui_web/`; being easier is not an argument.
- **`follow_cursor` is called from `on_action` and nowhere else.** `paint` renders from the core's
  stored `scroll_offset`. If painting followed the cursor, a wheel scroll away from the caret would
  snap back on the next `WM_PAINT` — the rule `refresh()` enforces in GTK and `render` in `ui_web/`.
- **The observer posts, it does not draw.** `HWND` is a raw pointer and so not `Send`, while
  `EditorObserver` must be `Send + Sync`, so `Notifier` carries the handle as an `isize` and uses
  only `PostMessageW`, which Microsoft documents as callable from any thread. An `AtomicBool`
  coalesces a burst into one paint, the same job the `async_channel` drain does in GTK.
- **Painting goes through a memory DC and one `BitBlt`,** and `WM_ERASEBKGND` returns 1. Painting
  text straight onto the window flickers visibly on every keystroke.
- **`ExtTextOutW` is given an explicit advance per glyph.** The caret arithmetic assumes one cell per
  character; the advance array forces the font to agree rather than hoping it does. Tabs are drawn
  as a single space for the same reason — the document keeps its tab.
- **The caret is a real `CreateCaret` caret**, not a painted rectangle: it brings the user's blink
  rate and width settings, and it is the only thing this window reports to assistive technology and
  to IMEs. `BeginPaint` hides it automatically, so nothing has to do that by hand.
- **Fonts and metrics are rebuilt on every `WM_DPICHANGED`**, and the DPI is read with
  `GetDpiForWindow` at `WM_CREATE` rather than assumed to be 96 — otherwise the window starts blurry
  on every scaled display. Per-monitor-v2 awareness is set by `SetProcessDpiAwarenessContext` in
  code, so the binary needs no manifest and therefore no build script.
- **`#![windows_subsystem = "windows"]`** makes it a GUI application, so there is no console flash
  from Explorer — and no stderr, which is why argument errors go into a `MessageBoxW`. **Never run
  this binary in CI**: a message box on a headless runner waits forever.
- **There is a menu bar, and it is the only pointer route to Save/Undo/Redo.** File (Save, Exit) and
  Edit (Undo, Redo), from `CreateMenu`/`AppendMenuW`, greyed at `WM_INITMENUPOPUP` from
  `is_dirty`/`can_undo`/`can_redo` — the same job the WinUI shell did by binding `IsEnabled`. Every
  item dispatches the same [`keymap::UiAction`] a keystroke produces, so there is one code path and
  the two cannot drift; Exit posts `WM_CLOSE` rather than closing, so it gets the same unsaved-changes
  dialog. The accelerator text after each tab is a *label* — the keys themselves live in `keymap.rs`,
  which is what keeps them testable.
- **Nothing may hold a `&mut App` across a modal dialog.** `MessageBoxW` runs its own message loop,
  so the window is repainted while the dialog is up and the window procedure is *re-entered* — and a
  second `app_from` there would alias the first `&mut`. `on_close` is a free function, handled
  before `app_from` in the dispatch, that borrows twice for as long as it takes to read a question
  and act on an answer. Any future modal — a file picker, a find bar — must copy that shape. This
  was a real bug, found by running the shell under Wine rather than by reading it.
- Known gaps, all deliberate: no Fluent controls, no Mica (a backdrop only shows through pixels the
  app does not paint, and this one paints them all), no accessibility beyond the caret, no
  scrollbar, no toolbar, no file dialog, no selection, no clipboard, no IME composition, no
  horizontal scrolling, and no Home/End/PageUp/PageDown/Delete — the core has no operation for any of those and
  inventing one in a shell is what the parity rule forbids.

## The macOS shell (`ui_mac/`)

SwiftUI over the same `ffi/` crate. **Swift bindings come from uniffi itself**, via a
`uniffi-bindgen` binary inside `ffi/`, so the generator is always on the same uniffi version as the
runtime. It is now the only generator here; the C# one, whose external binary had to be matched by
hand, went with the WinUI shell.

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
- **`FfiSmoke` is the FFI boundary's only smoke test now** — the 13 checks it used to share with
  `ffi/csharp-smoke`. It runs on Linux, so the boundary is still verifiable here; what is *not*
  verifiable locally is SwiftUI itself.
- Named constructors become static methods, not initialisers: `EditorHandle.open(path:)`, not
  `EditorHandle(path:)`. Only a constructor called `new` maps to `init`.
- Keys go through `.onKeyPress` (macOS 14+); ⌘-shortcuts are returned as `.ignored` so the menu bar
  handles them. `CommandGroup(replacing: .undoRedo)` is what stops AppKit's own undo stack — which
  knows nothing about the document — from taking ⌘Z.
- Known gap: no scrolling by mouse or trackpad; the viewport follows the caret only.

## The browser shell (`ui_web/`, crate `editor-web`)

Rust compiled to `wasm32-unknown-unknown`, depending on `core` directly and reaching the page
through **`wasm-bindgen`, not UniFFI** — which has no JavaScript target and would be the wrong tool
regardless, since the shell is Rust. That makes it a third class of shell: Rust-direct like the TUI,
foreign-bound like the Swift app, both at once.

Building is two steps, because rustc only produces half of what a page needs. `ui_web/build.sh`
compiles the `.wasm`, runs the `wasm-bindgen` CLI over it to write the JS glue, and copies
`index.html` and `style.css` into `ui_web/dist/` (gitignored). **The CLI's version and the
`wasm-bindgen` crate version must match exactly** — the build script reads the version out of
`Cargo.lock` and refuses to run otherwise, and `web.yml` installs the CLI the same way. This is the
same coupling as `uniffi` / `uniffi-bindgen-cs`, with the advantage that neither side has to be
pinned by hand.

- **The DOM is a renderer, not the document.** There is no `contenteditable` anywhere; `#text` is
  rebuilt from `get_viewport` every repaint and the caret is a positioned `<div>`. Exactly the GTK
  `TextView` and Win32 `EDIT` rule — if the browser is allowed to edit the text, it wins, and the
  core is no longer the source of truth.
- **The browser is the first platform here with no filesystem**, which is why `Editor::load_text`
  and `Editor::save_to_string` exist. A file arrives from the File API as a string and leaves as a
  download; the shell never sees a path, and the name it carries is only what the download is
  called. The page cannot write back to the file it opened, and that is the platform's rule, not a
  gap in the shell.
- **The observer holds a flag, not the page.** `EditorObserver` is `Send + Sync` and a wasm module is
  single-threaded, so `Notifier` owns an `AtomicBool` and schedules a `requestAnimationFrame`. The
  flag doubles as "a frame is already scheduled", which is what coalesces a burst into one repaint —
  the same job the GTK shell's channel drain does.
- **`render` draws from `scroll_offset` and never calls `follow_cursor`;** key handling calls it
  explicitly. Same reason as GTK: otherwise a wheel scroll away from the caret snaps straight back.
- **`keymap.rs` and `layout.rs` are DOM-free and unit-tested on the host.** `web-sys` compiles for
  any target, so `cargo test -p editor-web` needs no browser and no wasm toolchain. `layout.rs` is
  where the pixel arithmetic lives — caret placement, hit testing, wheel deltas — precisely so it can
  be tested at all.
- **Metrics come from `#probe`**, a hidden element carrying the same CSS as a line; its width is ten
  characters and its height one line. Re-measured every frame, because page zoom and a late font
  change both and neither fires an event. `Metrics::new` floors both at a non-zero value, or the
  first paint (before layout, when every rectangle is 0) would divide by zero.
- **`ui_web/smoke.js` is where the boundary is actually tested**, the sibling of `FfiSmoke`. `smoke.sh` regenerates the glue for the *node* target and drives the real module
  against the real `index.html` in jsdom: typing, arrows, undo/redo, Enter, the wheel, the status
  bar, a resize. jsdom has no layout engine, so every rectangle is zero and the viewport is one line
  tall — which is enough to prove the wiring, and means a failure after it passes is CSS.
  What it cannot see is how any of it looks: the layout was checked by hand in Firefox, and a real
  browser stays the only way to check it after a change to `style.css` or the caret arithmetic.
- Mouse-wheel scrolling exists here and not in the other GUI shells; it goes straight into
  `set_scroll_offset`, so the scroll rule is still the core's.
- Known gaps: no IME composition (a key that produces one non-control character is text, everything
  else is a named key), no touch keyboard on mobile (there is no input element to focus), and no
  scrollbar — the wheel and the caret are the only ways to move the view.

## The portable shell (`ui_egui/`, binary `edit-egui`)

`eframe`/`egui`, in immediate mode, and the only shell here that is **deliberately not native** —
decided on in [`doc/decision-egui-shell.md`](doc/decision-egui-shell.md), built in the stages of
[`doc/plan-egui-shell.md`](doc/plan-egui-shell.md). It is the one row of "Platform conventions" in
the checklist that is knowingly unmet, and the decision record says why. It buys two things no
native shell here offers: **no system dependencies on any platform**, and a behaviour test suite
that runs headlessly.

**egui is reached through `eframe::egui`**, never as a direct dependency — the same rule as
`ratatui::crossterm` and `libadwaita::gtk`. `egui_kittest` is the one version that rule cannot
police, because it is not re-exported: `eframe`, `egui` and `egui_kittest` must share a minor
version, so bump `egui_kittest` by hand whenever `eframe` moves.

- **`egui::Context` is the whole observer bridge.** It is `Clone + Send + Sync`, so `Notifier`
  (`main.rs`) holds one directly — no channel as in GTK, no `AtomicBool` as in the TUI and browser
  shells — and `request_repaint` coalesces by itself.
- **Nothing may ask for a repaint on a timer.** The core pushes; a continuous-repaint mode would
  make this the one shell that polls. `an_idle_shell_stops_asking_to_be_repainted` pins it.
- **No `TextEdit` and no `ScrollArea`, ever.** The first owns a `String` and the second a scroll
  position; the core owns both. This is the same rule that keeps the `GtkTextView` read-only and
  keeps `contenteditable` out of `ui_web/`. Immediate mode is not an exemption — both keep their
  state in egui's memory between frames.
- **The document is painted, so it carries its own `WidgetInfo`.** `Painter::text` produces no
  widget and therefore no accessibility node: without the `allocate_rect` + `WidgetInfo::labeled`
  in `draw_document`, a screen reader and the test harness both see an empty window.
  `the_document_is_announced_to_the_accessibility_tree` pins it, and it is what every other
  behaviour test queries — so breaking accessibility here breaks the suite, which is the right way
  round.
- **`App::frame(&mut Ui)` exists so the tests can drive the whole shell.** `eframe::App::ui` only
  delegates to it, because an `eframe::Frame` cannot be built outside eframe. Keep the logic in
  `frame`, or the harness stops seeing what the app really does.
- **Input is handled before painting, inside the same frame**, and `follow_cursor` is called from
  input handling only. In immediate mode both happen in one function, so the ordering is deliberate
  rather than structural: paint-then-scroll would make a wheel scroll away from the caret snap
  straight back, which is the rule `refresh()` enforces in GTK and `render` in `ui_web/`.
- **Metrics are re-measured every frame** (`Self::metrics`), because egui's zoom factor changes
  `glyph_width` and `row_height` and nothing announces it — the same reason `ui_web/` re-measures
  its `#probe`. `layout::Metrics::new` floors both at a non-zero value, or the first frame divides
  by zero.
- **The status line uses labels, not buttons.** This shell reads raw events rather than owning a
  focused text widget, so a focusable widget there would take Enter and the arrows away from the
  document.
- **Driving a bare `egui::Context` in a test panics on drop** unless `output.textures_delta` is
  cleared: a pass hands back textures the caller is supposed to upload. `egui_kittest` handles this,
  which is one more reason the behaviour tests go through the harness rather than the context.

Two API notes for 0.36, both of which invalidate anything written against an older egui:
`App::update(&mut self, ctx, frame)` is now `App::ui(&mut self, ui, frame)`, and `TopBottomPanel`
and `SidePanel` are gone, replaced by `egui::Panel::bottom(…)`. Read
`~/.cargo/registry/src/*/eframe-*/src/epi.rs` rather than trusting recall.

`keymap.rs` and `layout.rs` are widget-free and unit-tested, like their `ui_web/` counterparts.
`app.rs`'s tests are the interesting ones: they drive the real shell through `egui_kittest` with no
display and no GPU, and they are what makes this the first GUI here whose behaviour CI checks. They
prove **no pixels** — the harness stops before rasterising — so look at the window after a change to
how it looks. That caveat is written at the test module.

They live in `src/app.rs` and not in the `tests/behaviour.rs` the plan named, because this crate has
only a `[[bin]]` target and an integration test has no library to link against. Adding a `lib.rs`
purely to move them would be the wrong trade; if the file ever grows too large, split the shell into
`lib.rs` + a thin `main.rs` deliberately rather than as a side effect.

Known gaps, all deliberate: no scrollbar, no file dialog (the path comes from `argv`, as in
`edit-tui` and `edit-gtk`, which is why this shell needed no new core capability), no selection, no
clipboard, no IME, no horizontal scrolling, and no egui-on-wasm build — `ui_web/` is the browser
shell and a canvas would be a worse one.

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
