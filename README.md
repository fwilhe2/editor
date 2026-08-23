# editor

A prototype of the **Shared Core, Native Shell** architecture: one Rust library holds all the
editing logic, and six front-ends render it — a CLI, a terminal UI, a GTK4/GNOME app, a WinUI 3 app,
a SwiftUI app, and a browser app compiled to WebAssembly.

It is a text editor only incidentally. The feature set is deliberately tiny — insert, backspace,
cursor movement, undo/redo, save — because the point is not the editor. The point is that one core
drives six very different UIs across three operating systems and the web, two of them across an FFI
boundary, without any of them owning a byte of document state.

**This is a work in progress.** The argument below is the reason it exists.

[![core + cli + ffi](https://github.com/fwilhe2/editor/actions/workflows/core-cli.yml/badge.svg)](https://github.com/fwilhe2/editor/actions/workflows/core-cli.yml)
[![ui_tui](https://github.com/fwilhe2/editor/actions/workflows/tui.yml/badge.svg)](https://github.com/fwilhe2/editor/actions/workflows/tui.yml)
[![ui_linux](https://github.com/fwilhe2/editor/actions/workflows/linux.yml/badge.svg)](https://github.com/fwilhe2/editor/actions/workflows/linux.yml)
[![ui_windows](https://github.com/fwilhe2/editor/actions/workflows/windows.yml/badge.svg)](https://github.com/fwilhe2/editor/actions/workflows/windows.yml)
[![ui_mac](https://github.com/fwilhe2/editor/actions/workflows/macos.yml/badge.svg)](https://github.com/fwilhe2/editor/actions/workflows/macos.yml)
[![ui_web](https://github.com/fwilhe2/editor/actions/workflows/web.yml/badge.svg)](https://github.com/fwilhe2/editor/actions/workflows/web.yml)

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
 edit       CLI                  editor-web           EditorApp (C#)
 edit-tui   terminal             wasm · DOM           WinUI 3 · Windows
 edit-gtk   GTK4 / GNOME                              EditorApp (Swift)
 ui_qt      planned                                   SwiftUI · macOS
```

Three classes of shell, and the difference matters:

- **Rust shells** — the CLI, TUI and GTK app — depend on `editor-core` as an ordinary Cargo
  dependency and call its public API directly. No bindings, no translation layer.
- **Foreign shells** — WinUI and SwiftUI — go through `editor-ffi`, a thin UniFFI facade that
  generates C# and Swift bindings. The annotations live in their own crate so the core's Rust API
  stays idiomatic (`impl AsRef<Path>`, `PathBuf`, `char`) instead of being flattened into strings
  for the benefit of foreign callers.
- **The browser shell** — Rust again, compiled to `wasm32-unknown-unknown` and bound to the page
  with `wasm-bindgen`. UniFFI has no JavaScript target, and would be beside the point when the shell
  is Rust: `editor-ffi` is not involved at all.

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
| `ui_web/` | `editor-web` — WebAssembly app rendered into the DOM, plus a jsdom smoke test |

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

### Linux GUI — GTK4 + libadwaita

```sh
sudo apt install libgtk-4-dev libadwaita-1-dev     # Debian/Ubuntu
# sudo dnf install gtk4-devel libadwaita-devel     # Fedora

cargo run --release -p editor-gtk -- somefile.txt
```

### Windows — WinUI 3

Needs the .NET 8 SDK and the Windows App SDK (the "Windows application development" workload in
Visual Studio installs both). The C# bindings are generated, not committed, so generate them first:

On a fresh Windows installation, run PowerShell as Administrator and use the unattended,
idempotent bootstrapper. Add `-Build` to perform the complete build after installation:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\install-windows-dependencies.ps1 -Build
```

The script installs Visual Studio Build Tools with the MSVC compiler and Windows 11 SDK, the .NET
8 SDK, Rust's stable MSVC toolchain, and the pinned UniFFI C# generator. It downloads installers
directly from their upstream vendors and skips components that are already present.

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

> The CLI's version must equal the `wasm-bindgen` crate's, or the glue will not match the module —
> the same coupling as `uniffi` and `uniffi-bindgen-cs`, except `build.sh` reads the version out of
> `Cargo.lock` and refuses to run on a mismatch, so there is nothing to pin by hand.

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
# C#
LD_LIBRARY_PATH=target/release dotnet run --project ffi/csharp-smoke

# Swift
swift build --package-path ui_mac --product FfiSmoke \
  -Xlinker "$PWD/target/release/libeditor_ffi.a"
./ui_mac/.build/debug/FfiSmoke

# Browser: the real wasm module driven against the real page in jsdom, no browser
./ui_web/smoke.sh release
```

All three run before their app build in CI, so a failure tells you immediately whether the bug is in
the bindings or in the UI code.

## Status and limits

This is a prototype, and it is honest about being one. Known gaps:

- No horizontal scrolling in the TUI; no mouse-wheel scrolling in the desktop GUI shells (the
  browser shell does have it).
- The browser shell has no IME composition, no touch keyboard on mobile, and no scrollbar. A page
  also cannot write back to the file it opened — saving is a download, which is the platform's rule.
- Line endings are assumed to be LF.
- No search, selection, clipboard, multiple documents or syntax highlighting.

One more shell is planned, and it stretches the architecture in a useful direction:

- **Qt** (`ui_qt/`) — a second desktop toolkit, and the KDE/Plasma conventions that come with it.
  Likely via [`cxx-qt`](https://github.com/KDAB/cxx-qt), which would keep it a plain Rust crate
  depending on the core directly, like the GTK and terminal shells. See `CLAUDE.md` for the
  trade-off against a C++ Qt app, which would need a fourth binding mechanism.

The browser shell (`ui_web/`) was the previous entry on that list. It is the honest test of the
argument above — the web is one more platform with conventions of its own, not an excuse to stop
having any — and it proved the two boundaries that mattered: `get_viewport` survives a DOM renderer
unchanged, and the core's file API grew a filesystem-free half (`load_text` / `save_to_string`) that
the CLI exposes as `import` / `export`, because a capability in one shell alone is a bug.

`CLAUDE.md` documents the architecture in more depth, including the invariants worth preserving and
the traps each shell hides. [`doc/shared-core-native-shell.md`](doc/shared-core-native-shell.md)
generalises it into a guide for building an app this way from scratch — the rules that carry the
weight, the order to build in, and how to verify each shell from a Linux or macOS host.

## License

MIT — see [LICENSE](LICENSE).
