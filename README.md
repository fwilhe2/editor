# editor

A prototype of the **Shared Core, Native Shell** architecture: one Rust library holds all the
editing logic, and seven front-ends render it — a CLI, a terminal UI, a GTK4/GNOME app, a Win32 app,
a SwiftUI app, a browser app compiled to WebAssembly, and a portable GUI that is native to nothing.

It is a text editor only incidentally. The feature set is deliberately tiny — insert, backspace,
cursor movement, undo/redo, save — because the point is not the editor. The point is that one core
drives seven very different UIs across three operating systems and the web, one of them across an
FFI boundary, without any of them owning a byte of document state.

**This is a work in progress.** The argument below is the reason it exists.

[![core + cli + ffi](https://github.com/fwilhe2/editor/actions/workflows/core-cli.yml/badge.svg)](https://github.com/fwilhe2/editor/actions/workflows/core-cli.yml)
[![ui_tui](https://github.com/fwilhe2/editor/actions/workflows/tui.yml/badge.svg)](https://github.com/fwilhe2/editor/actions/workflows/tui.yml)
[![ui_linux](https://github.com/fwilhe2/editor/actions/workflows/linux.yml/badge.svg)](https://github.com/fwilhe2/editor/actions/workflows/linux.yml)
[![ui_win32](https://github.com/fwilhe2/editor/actions/workflows/win32.yml/badge.svg)](https://github.com/fwilhe2/editor/actions/workflows/win32.yml)
[![ui_mac](https://github.com/fwilhe2/editor/actions/workflows/macos.yml/badge.svg)](https://github.com/fwilhe2/editor/actions/workflows/macos.yml)
[![ui_web](https://github.com/fwilhe2/editor/actions/workflows/web.yml/badge.svg)](https://github.com/fwilhe2/editor/actions/workflows/web.yml)
[![ui_egui](https://github.com/fwilhe2/editor/actions/workflows/egui.yml/badge.svg)](https://github.com/fwilhe2/editor/actions/workflows/egui.yml)

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

## How it fits together

```
                        ┌───────────────────────────┐
                        │        editor-core        │
                        │ rope · cursor · undo/redo │
                        │ the only source of truth  │
                        └─────────────┬─────────────┘
                                      │
        ┌─────────────────────────────┼───────────────────────┐
        │                             │                       │
Cargo dependency                wasm-bindgen               UniFFI
 (no FFI at all)          (a Cargo dependency too)      (editor-ffi)
        │                             │                       │
 edit       CLI                  editor-web           EditorApp (Swift)
 edit-tui   terminal             wasm · DOM           SwiftUI · macOS
 edit-gtk   GTK4 / GNOME
 edit-win32 Win32 · Windows
 edit-egui  portable
 ui_qt      planned
```

Three classes of shell, and the difference matters:

- **Rust shells** — the CLI, TUI, GTK app, Win32 app and the egui one — depend on `editor-core` as
  an ordinary Cargo dependency and call its public API directly. No bindings, no translation layer.
- **Foreign shells** — SwiftUI, and since the C# app was replaced only SwiftUI — go through
  `editor-ffi`, a thin UniFFI facade that generates Swift bindings. The annotations live in their
  own crate so the core's Rust API stays idiomatic (`impl AsRef<Path>`, `PathBuf`, `char`) instead
  of being flattened into strings for the benefit of foreign callers.
- **The browser shell** — Rust again, compiled to `wasm32-unknown-unknown` and bound to the page
  with `wasm-bindgen`. UniFFI has no JavaScript target, and would be beside the point when the shell
  is Rust: `editor-ffi` is not involved at all.

Rules the whole design leans on:

- The text is a **rope**, and `get_viewport(start, end)` is the **only** way a shell reads it. No UI
  ever holds the whole document.
- **Undo/redo lives in the core** as a command pattern, so every shell gets identical history.
- The core **pushes** changes out through an observer trait; shells never poll. In Swift it is a
  UniFFI foreign trait; in the Win32 shell it is a posted window message; in the browser, a flag and
  a `requestAnimationFrame`.
- **Feature parity is a hard rule**: anything reachable from any GUI must also be reachable from the
  CLI. A UI-only feature is a bug.

## Layout

| Path | What it is |
|------|-----------|
| `core/` | `editor-core` — the logic. Rope, cursor, viewport, undo history |
| `cli/` | `edit` — scriptable front-end, and the thing that keeps parity honest |
| `ffi/` | `editor-ffi` — UniFFI facade for the Swift shell |
| `ui_tui/` | `edit-tui` — terminal UI (ratatui + crossterm) |
| `ui_linux/` | `edit-gtk` — GTK4 + libadwaita, following the GNOME HIG |
| `ui_win32/` | `edit-win32` — Win32 + GDI, depending on nothing Windows does not ship |
| `ui_mac/` | SwiftUI app, following Apple's HIG, plus a Swift smoke test |
| `ui_web/` | `editor-web` — WebAssembly app rendered into the DOM, plus a jsdom smoke test |
| `ui_egui/` | `edit-egui` — portable GUI on egui/eframe, native to nothing, and the only one with headless behaviour tests |

## Building

**Cross-compilation is not supported anywhere in this project, by design.** Each app is built on the
OS it runs on, and CI does the same on Linux, Windows and macOS runners.

Everything needs a Rust toolchain. There is no minimum version to respect: the project builds on
current stable, which is what CI tests.

### CLI and TUI — Linux, macOS, Windows

Pure Rust, no system dependencies:

```sh
cargo build --release -p editor-cli -p editor-tui

./target/release/edit --help
./target/release/edit-tui somefile.txt
```

### Every platform — egui

Also pure Rust, and the only GUI with no system dependencies on any platform: Linux, macOS and
Windows all build it with nothing but a Rust toolchain.

```sh
cargo run --release -p editor-egui -- somefile.txt
```

It edits: arrows move, typing inserts, Ctrl/⌘+Z and +Y undo and redo, +S saves, +Q quits (twice if
there are unsaved changes), the wheel scrolls and a click places the caret.

It is also the one GUI here whose behaviour is *tested* rather than merely compiled — 33 tests that
drive the real shell, headlessly, with no display and no GPU:

```sh
cargo test -p editor-egui       # needs no window, and passes without one
```

Those run on all three OS runners in CI, which is why this is the first GUI in the repository whose
badge above means more than "it still compiles".

### Linux GUI — GTK4 + libadwaita

```sh
sudo apt install libgtk-4-dev libadwaita-1-dev     # Debian/Ubuntu
# sudo dnf install gtk4-devel libadwaita-devel     # Fedora

cargo run --release -p editor-gtk -- somefile.txt
```

### Windows — Win32

Needs rustup with the MSVC toolchain, and Visual Studio Build Tools for the linker. That is the
whole list — no .NET, no Windows App SDK, no bindings to generate:

```powershell
cargo build --release -p editor-win32
target\release\edit-win32.exe somefile.txt
```

The resulting executable **depends on nothing Windows does not already ship**. It links `user32`,
`gdi32`, `dwmapi` and `advapi32`, and `.cargo/config.toml` links the MSVC C runtime statically so
there is no Visual C++ redistributable to install either. CI reads the import table back and fails
if anything else appears.

This shell used to be a WinUI 3 application in C#; [`doc/decision-win32-shell.md`](doc/decision-win32-shell.md)
records why it was replaced and what that cost — chiefly the project's only C# binding.

> Its Windows-only source can be **type-checked without Windows**, because `cargo check` never
> links. On any machine: `rustup target add x86_64-pc-windows-msvc && cargo check -p editor-win32
> --target x86_64-pc-windows-msvc`. This is a checking convenience, not cross-compilation — the
> `.exe` is still built on Windows.

### macOS — SwiftUI

Needs Xcode 15 or newer (Swift 5.9+) and macOS 14+:

```sh
cargo build --release -p editor-ffi
./ui_mac/generate-bindings.sh release

swift build --package-path ui_mac --product EditorApp -c release \
  -Xlinker "$PWD/target/release/libeditor_ffi.a"
```

It is a Swift package rather than an `.xcodeproj`, so Xcode can open `ui_mac/` directly. The app
links the Rust **static** library, so the bundle has nothing to locate at runtime. CI wraps the
resulting executable in an `Editor.app` using `ui_mac/Info.plist`.

### Browser — WebAssembly

The one shell that is not tied to an operating system, and the one that needs two build steps:
rustc produces the `.wasm`, and the `wasm-bindgen` CLI writes the JavaScript that loads it.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version "$(grep -A1 '^name = "wasm-bindgen"$' Cargo.lock \
  | sed -n 's/^version = "\(.*\)"/\1/p' | head -n1)"

./ui_web/build.sh release
python3 -m http.server --directory ui_web/dist 8000
```

> The CLI's version must equal the `wasm-bindgen` crate's, or the glue will not match the module.
> Unlike the `uniffi-bindgen-cs` pin this replaced — deleted along with the C# shell — `build.sh`
> reads the version out of `Cargo.lock` and refuses to run on a mismatch, so there is nothing to
> pin by hand.

Serve it; do not open `ui_web/dist/index.html` from disk, because browsers refuse to load ES modules
and `.wasm` over `file://`. There is no server component — the output is four static files.

## Using the CLI

The CLI exists so that everything the GUIs can do is also scriptable — useful for agents, shells and
CI. It is non-interactive: stdout is parseable, diagnostics go to stderr, failures exit non-zero.
**Lines and columns are 1-based**, and column 1 is before the first character.

```sh
edit new notes.txt
edit insert notes.txt --text 'hello' --line 1 --col 1
edit view notes.txt
edit --format json info notes.txt
```

`export` and `import` are the whole document rather than a range of lines — the scriptable half of
what the browser shell does when it opens a file and saves it back into a download:

```sh
edit export notes.txt > backup.txt        # byte for byte, unlike `view`
edit import notes.txt --text - < backup.txt
```

Each invocation loads the file, applies one command and writes it back. Because the process is
stateless and the editor is not, cursor position and undo history survive only via a session file:

```sh
edit --session .notes.session insert notes.txt --text 'x'
edit --session .notes.session undo notes.txt
```

Without `--session`, `undo` and `redo` fail loudly rather than silently doing nothing.

## Tests

```sh
cargo test --workspace     # needs the GTK dev packages on Linux for ui_linux
```

The boundaries to other languages and to the browser are covered by smoke tests that are
deliberately UI-free, so they run on any OS — including the one that cannot build the shell they
belong to:

```sh
# Swift
swift build --package-path ui_mac --product FfiSmoke \
  -Xlinker "$PWD/target/release/libeditor_ffi.a"
./ui_mac/.build/debug/FfiSmoke

# Browser: the real wasm module driven against the real page in jsdom, no browser
./ui_web/smoke.sh release
```

Both run before their app build in CI, so a failure tells you immediately whether the bug is in the
bindings or in the UI code. The Win32 shell needs no such harness: it has no boundary, and its
windowless modules are tested directly with `cargo test -p editor-win32` on any host.

The egui shell goes further: its tests drive the real app, not a boundary beside it. They run the
whole egui pass — the same layout and text shaping the window uses — feed it synthetic keys, clicks
and wheel events, and read back the accessibility tree, all with no display and no GPU:

```sh
env -u DISPLAY -u WAYLAND_DISPLAY cargo test -p editor-egui
```

They stop before rasterising, so they prove behaviour and prove nothing about pixels. That is the
same caveat jsdom carries, and it points the same way: after a change to how a UI *looks*, open it.

## Status and limits

This is a prototype, and it is honest about being one. Known gaps:

- No horizontal scrolling in the TUI; no mouse-wheel scrolling in the SwiftUI shell (the browser,
  egui and Win32 ones do have it).
- The Win32 shell has a plain menu bar rather than Fluent controls, no Mica, and no accessibility
  beyond the system caret — the deliberate price of an executable with no runtime dependencies.
- The browser shell has no IME composition, no touch keyboard on mobile, and no scrollbar. A page
  also cannot write back to the file it opened — saving is a download, which is the platform's rule.
- The egui shell has no scrollbar, no file dialog and no IME; its path comes from `argv`, like the
  terminal and GTK ones.
- Line endings are assumed to be LF.
- No search, selection, clipboard, multiple documents or syntax highlighting.

One more shell is on the way:

- **Qt** (`ui_qt/`) — planned: a second desktop toolkit, and the KDE/Plasma conventions that come
  with it. Likely via [`cxx-qt`](https://github.com/KDAB/cxx-qt), which would keep it a plain Rust
  crate depending on the core directly, like the GTK and terminal shells. See `CLAUDE.md` for the
  trade-off against a C++ Qt app, which would need a fourth binding mechanism. Note that egui does
  not replace it and the two have opposite purposes: Qt exists to reach a *second* set of native
  conventions, egui to reach none of them.

The two shells that came off that list most recently are finished, and each stretched the
architecture in a different direction.

The **browser shell** (`ui_web/`) is the honest test of the argument above — the web is one more
platform with conventions of its own, not an excuse to stop having any — and it proved the two
boundaries that mattered: `get_viewport` survives a DOM renderer unchanged, and the core's file API
grew a filesystem-free half (`load_text` / `save_to_string`) that the CLI exposes as
`import` / `export`, because a capability in one shell alone is a bug.

The **egui shell** (`ui_egui/`) is the one that is **deliberately not native**. It looks and behaves
the same everywhere, which is exactly the compromise the argument above is against; it was built
anyway, for two things no native shell here can offer. It needs no system dependencies on any
platform — a Rust toolchain is the whole list — and its behaviour is *tested* rather than merely
compiled, headlessly, with no display and no GPU. That makes it the first GUI in this repository
that CI runs, and the first an agent can verify without a human looking at a screen. It cost the core
nothing: no new capability, no new API, and therefore no new CLI subcommand — the seventh shell was a
transcription of the first six.

It is also the control group the argument above was missing. Having one portable shell built from the
same core in the same style is what makes "native feels different" a claim you can check rather than
assert — run `edit-egui` and `edit-gtk` side by side and the difference is the argument. What it does
not do is weaken it: the tests prove behaviour, never appearance, and behaviour is not the part
platform conventions are about. See [`doc/decision-egui-shell.md`](doc/decision-egui-shell.md) for
why that trade is worth making and [`doc/plan-egui-shell.md`](doc/plan-egui-shell.md) for how it was
built.

`CLAUDE.md` documents the architecture in more depth, including the invariants worth preserving and
the traps each shell hides. [`doc/shared-core-native-shell.md`](doc/shared-core-native-shell.md)
generalises it into a guide for building an app this way from scratch — the rules that carry the
weight, the order to build in, and how to verify each shell from a Linux or macOS host.

## License

MIT — see [LICENSE](LICENSE).
