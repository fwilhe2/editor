# editor

A prototype of the **Shared Core, Native Shell** architecture: one Rust library holds all the
editing logic, and six front-ends render it — a CLI, a terminal UI, a GTK4/GNOME app, a WinUI 3 app,
and a SwiftUI app.

It is a text editor only incidentally. The feature set is deliberately tiny — insert, backspace,
cursor movement, undo/redo, save — because the point is not the editor. The point is that one core
drives five very different UIs across three operating systems, two of them across an FFI boundary,
without any of them owning a byte of document state.

**This is a work in progress.** The argument below is the reason it exists.

[![core + cli + ffi](https://github.com/fwilhe2/editor/actions/workflows/core-cli.yml/badge.svg)](https://github.com/fwilhe2/editor/actions/workflows/core-cli.yml)
[![ui_tui](https://github.com/fwilhe2/editor/actions/workflows/tui.yml/badge.svg)](https://github.com/fwilhe2/editor/actions/workflows/tui.yml)
[![ui_linux](https://github.com/fwilhe2/editor/actions/workflows/linux.yml/badge.svg)](https://github.com/fwilhe2/editor/actions/workflows/linux.yml)
[![ui_windows](https://github.com/fwilhe2/editor/actions/workflows/windows.yml/badge.svg)](https://github.com/fwilhe2/editor/actions/workflows/windows.yml)
[![ui_mac](https://github.com/fwilhe2/editor/actions/workflows/macos.yml/badge.svg)](https://github.com/fwilhe2/editor/actions/workflows/macos.yml)

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
                          │  rope · cursor · undo/redo │
                          │  the only source of truth  │
                          └─────────────┬─────────────┘
                                        │
              ┌─────────────────────────┼──────────────────────┐
              │  Cargo dependency       │      UniFFI          │
              │  (no FFI at all)        │      (editor-ffi)    │
     ┌────────┴────────┬───────────┐    │    ┌─────────────────┴────────┐
     │        │        │           │    │    │                          │
   edit    edit-tui  edit-gtk      │    │  EditorApp (C#)      EditorApp (Swift)
   CLI      TUI      GTK4/GNOME    │    │  WinUI 3 · Windows   SwiftUI · macOS
                                   │    │
                    planned: ui_qt ┘    └ planned: ui_web (wasm)
```

Two classes of shell, and the difference matters:

- **Rust shells** — the CLI, TUI and GTK app — depend on `editor-core` as an ordinary Cargo
  dependency and call its public API directly. No bindings, no translation layer.
- **Foreign shells** — WinUI and SwiftUI — go through `editor-ffi`, a thin UniFFI facade that
  generates C# and Swift bindings. The annotations live in their own crate so the core's Rust API
  stays idiomatic (`impl AsRef<Path>`, `PathBuf`, `char`) instead of being flattened into strings
  for the benefit of foreign callers.

Rules the whole design leans on:

- The text is a **rope**, and `get_viewport(start, end)` is the **only** way a shell reads it. No UI
  ever holds the whole document.
- **Undo/redo lives in the core** as a command pattern, so every shell gets identical history.
- The core **pushes** changes out through an observer trait; shells never poll. In Swift and C# this
  is a UniFFI foreign trait implemented in the shell's own language.
- **Feature parity is a hard rule**: anything reachable from any GUI must also be reachable from the
  CLI. A UI-only feature is a bug.

## Layout

| Path | What it is |
|------|-----------|
| `core/` | `editor-core` — the logic. Rope, cursor, viewport, undo history |
| `cli/` | `edit` — scriptable front-end, and the thing that keeps parity honest |
| `ffi/` | `editor-ffi` — UniFFI facade, plus a C# smoke test of the boundary |
| `ui_tui/` | `edit-tui` — terminal UI (ratatui + crossterm) |
| `ui_linux/` | `edit-gtk` — GTK4 + libadwaita, following the GNOME HIG |
| `ui_windows/` | WinUI 3 app in C#, following Microsoft's Fluent guidance |
| `ui_mac/` | SwiftUI app, following Apple's HIG, plus a Swift smoke test |

## Building

**Cross-compilation is not supported anywhere in this project, by design.** Each app is built on the
OS it runs on, and CI does the same on Linux, Windows and macOS runners.

Everything needs a Rust toolchain. The MSRV is **1.85** and dependencies are pinned to respect it.

### CLI and TUI — Linux, macOS, Windows

Pure Rust, no system dependencies:

```sh
cargo build --release -p editor-cli -p editor-tui

./target/release/edit --help
./target/release/edit-tui somefile.txt
```

### Linux GUI — GTK4 + libadwaita

```sh
sudo apt install libgtk-4-dev libadwaita-1-dev     # Debian/Ubuntu
# sudo dnf install gtk4-devel libadwaita-devel     # Fedora

cargo run --release -p editor-gtk -- somefile.txt
```

### Windows — WinUI 3

Needs the .NET 8 SDK and the Windows App SDK (the "Windows application development" workload in
Visual Studio installs both). The C# bindings are generated, not committed, so generate them first:

```powershell
cargo build --release -p editor-ffi

cargo install uniffi-bindgen-cs `
  --git https://github.com/NordSecurity/uniffi-bindgen-cs --tag v0.11.0+v0.31.0
uniffi-bindgen-cs --library target/release/editor_ffi.dll --out-dir ui_windows/Generated

dotnet build ui_windows/EditorApp.csproj -c Release -p:Platform=x64
```

The app is unpackaged (no MSIX), and `editor_ffi.dll` is copied next to the executable by the build.

> The `uniffi-bindgen-cs` tag and the `uniffi` version in `Cargo.toml` are a matched pair. The
> generator lags upstream uniffi, so uniffi's newest release is usually *not* the one to use.

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

The two FFI boundaries are covered by smoke tests that are deliberately UI-free, so they run on any
OS — including the one that cannot build the shell they belong to:

```sh
# C#
LD_LIBRARY_PATH=target/release dotnet run --project ffi/csharp-smoke

# Swift
swift build --package-path ui_mac --product FfiSmoke \
  -Xlinker "$PWD/target/release/libeditor_ffi.a"
./ui_mac/.build/debug/FfiSmoke
```

Both run before the app build in CI, so a failure tells you immediately whether the bug is in the
bindings or in the UI code.

## Status and limits

This is a prototype, and it is honest about being one. Known gaps:

- No horizontal scrolling in the TUI; no mouse-wheel scrolling in the GUI shells.
- Line endings are assumed to be LF.
- No search, selection, clipboard, multiple documents or syntax highlighting.

Two more shells are planned, both of which stretch the architecture in a useful direction:

- **Qt** (`ui_qt/`) — a second desktop toolkit, and the KDE/Plasma conventions that come with it.
  Likely via [`cxx-qt`](https://github.com/KDAB/cxx-qt), which would keep it a plain Rust crate
  depending on the core directly, like the GTK and terminal shells. See `CLAUDE.md` for the
  trade-off against a C++ Qt app, which would need a third binding mechanism.
- **Browser / WebAssembly** (`ui_web/`) — the core is pure Rust with no platform assumptions, so it
  compiles to `wasm32-unknown-unknown` as-is. A web shell would reach it through `wasm-bindgen`
  rather than UniFFI, adding a third class of shell and proving the same viewport API works when the
  UI is a DOM. It is also the honest test of the argument above: the web is one more platform with
  conventions of its own, not an excuse to stop having any.

`CLAUDE.md` documents the architecture in more depth, including the invariants worth preserving and
the traps each shell hides.

## License

MIT — see [LICENSE](LICENSE).
