# Building an app as a shared core with native shells

A guide for agents. This repository is the worked example — one Rust core (`core/`) driving seven
front-ends: a CLI, a terminal UI, GTK4/GNOME, WinUI 3, SwiftUI, a WebAssembly browser app, and one
portable GUI that is native to nothing. What follows is what the pattern actually demands, in the
order you need it, with the traps that cost time and the checks that catch them.

The economics are the point. A genuinely native UI per platform used to mean paying for a different
language, toolkit and build system three or four times over, which almost nobody could justify. That
per-platform labour is now cheap. What stays expensive is judgement: what the core owns, where the
boundaries go, and what each platform's conventions actually are. This document is about spending
your effort there.

## 1. The shape

```
                        one core crate: all state, all logic, all I/O
                                          │
        ┌─────────────────────────────────┼─────────────────────────┐
   Rust shells                     wasm-bindgen shell        foreign shells
   (Cargo dependency)              (Cargo dependency,        (generated bindings
                                    bound to the DOM)         from a facade crate)
   CLI · TUI · GTK · egui · Qt          browser              Swift · C# · Kotlin
```

Three classes, and knowing which one you are writing decides everything else:

| Class | Reaches the core by | Generated code | Version coupling to watch |
|---|---|---|---|
| Rust-direct | ordinary `use editor_core::…` | none | none |
| wasm-bindgen | Cargo dependency, compiled to `wasm32-unknown-unknown` | JS glue from the `wasm-bindgen` **CLI** | CLI version must equal the crate version, exactly |
| UniFFI foreign | a separate facade crate (`ffi/`) | Swift / C# / Kotlin bindings | each language's bindgen lags upstream UniFFI |

A shell is a renderer and an event forwarder. It holds **no** application state. If you find
yourself adding a field to a shell that is not a presentation concern (a status message, a "confirm
quit" flag), the core is missing something.

## 2. The rules that carry the weight

Break any of these and the pattern quietly stops paying.

1. **One source of truth, and it is never the toolkit.** Every mature UI toolkit ships a text widget
   that owns a document: `GtkTextView`, `TextBox`, `QPlainTextEdit`, `contenteditable`,
   `egui::TextEdit`. Make it read-only and refill it from the core on every repaint — or, where the
   toolkit lets you paint text yourself, do not use the widget at all. Let it edit itself and you
   have two documents that disagree, and the toolkit will win the argument. Immediate mode is no
   exemption: `egui::TextEdit` keeps a `String` and a cursor in the framework's own memory between
   frames, which is the same trap wearing different clothes.
2. **Reads go through a windowed API.** Here that is `get_viewport(start, end)`. Never expose a
   getter that hands a shell the whole document; the moment one exists, three shells will use it and
   the data structure underneath stops mattering. (Saving is the one honest exception — see rule 8.)
3. **Undo/redo lives in the core**, as a command pattern with the inverse falling out of the action
   itself. Seven shells cannot each implement history and stay consistent.
4. **The core pushes, shells never poll.** Declare an observer trait in Rust; implement it in Rust
   directly, as a UniFFI foreign trait in Swift/C#, and as a flag-plus-repaint in wasm.
5. **Whatever any GUI can do, the CLI can do.** This is the ratchet that keeps capabilities out of
   shells. Adding a feature means: core first, CLI subcommand in the same change, then the UI. A
   UI-only feature is a bug, and it is the bug that eventually turns one core into seven.
6. **Shells must diverge.** Menu bar placement, redo shortcuts, close-confirmation, colour scheme —
   consult each platform's current guidelines rather than porting your first shell's habits. If all
   your UIs look alike you have paid for native and shipped cross-platform.
7. **Shape the core's public API for Rust, and put the FFI annotations somewhere else.** `impl
   AsRef<Path>`, `PathBuf`, `char` and generics do not cross an FFI boundary. Exporting the core
   directly means degrading it for every Rust caller; a thin facade crate whose methods forward
   one-to-one keeps both sides idiomatic and gives behaviour nowhere to drift.
8. **Do not assume a filesystem.** The browser has none: a file arrives as a string from the File API
   and leaves as a download. Pair `load_file`/`save_file` with `load_text(name, text)` /
   `save_to_string(name)` from the start. Note that a load is *not* a big insert — it must reset the
   cursor and clear the history, or undo will resurrect text the document never had.
9. **Never cross-compile the apps.** Each shell builds on the OS it runs on, in its own CI job. wasm
   is not an exception to this rule, it is outside it: `wasm32-unknown-unknown` *is* the target it
   runs on, so one Linux job builds the artifact every platform gets. A *portable* toolkit is not an
   exception either, and it is the one most likely to look like one: a shell that runs everywhere
   still gets a build per OS, because what a portable toolkit centralises is **verification**, not
   distribution — see the egui row in §5.

## 3. Order of work

Build in this order. Each step makes the next one cheaper, and the early steps are where design
mistakes are still free.

1. **Core, with tests, no UI at all.** Get the data structure, the action/undo model and the
   viewport API right. Write the deadlock test (see §6) now, not later.
2. **The CLI.** It is the cheapest complete front-end and it forces the API to be finished:
   stateless invocations expose everything the core forgot to expose. It is also how agents and CI
   drive the app forever after. Decide the 0-based/1-based question here and convert in exactly one
   file.
3. **The TUI.** The cheapest shell with a real event loop, and the one that first needs scrolling,
   repaint coalescing and lifecycle teardown. Whatever you invent here that a second shell would
   need — the scroll rule, for instance — move into the core *before* writing that second shell.
4. **One native desktop shell** (GTK, WinUI or SwiftUI). The first one is where you learn how much
   of the shell is boilerplate you can copy. Keep the key mapping in a widget-free module.
5. **The facade crate**, then the remaining foreign shells, one per OS, each with a smoke test
   (§5) and its own workflow.
6. **The browser shell last** if you want the strongest test of the boundary: a DOM renderer is the
   furthest thing from your data structure, and it has no filesystem to lean on.

There is a case for one more, and for building it earlier than its usefulness suggests: **a portable
immediate-mode shell** (egui here), which is the only GUI whose behaviour a machine can check (§5).
It is not a platform, so it does not extend the argument for going native — but it needs no system
dependencies, it makes every later refactor of the core verifiable through a real UI, and it is the
control group that lets "native feels different" be demonstrated rather than asserted. Build it
after the first native shell, so you already know what you are comparing against.

## 4. Shapes worth copying

These recur in every shell. Write them the same way each time; the repetition is the point, because
it makes the seventh shell a transcription rather than a design problem.

**A widget-free keymap module.** Key plus modifiers in, `UiAction` out; a second function applies the
action to the core and returns whatever the *shell* must handle (save, quit, open a picker).

```rust
pub fn action_for(key: Key, mods: Modifiers) -> Option<UiAction>;
pub fn apply(action: UiAction, editor: &Editor) -> Option<Request>;
```

It unit-tests with no display, no terminal and no browser, which in a retained-mode shell makes it
the only part you can test cheaply — so put everything decidable there. (`ui_linux/src/keymap.rs`,
`ui_web/src/keymap.rs`, `ui_egui/src/keymap.rs`.) Write it this way even in an immediate-mode shell,
where the whole app is testable: a failing unit test names the rule that broke, and a failing
end-to-end one only tells you the app is wrong.

**A pixel-arithmetic module** for shells that place a caret themselves: metrics in, positions out.
Caret placement, click hit-testing and wheel-delta conversion are pure functions of two measured
numbers; kept separate they are testable, and inline they are not. (`ui_web/src/layout.rs`.)

**An observer bridge per toolkit.** The core's observer is `Send + Sync`; most UI objects are
neither. The bridge is always some variant of "receive the notification anywhere, repaint on the UI
thread":

| Shell | Bridge |
|---|---|
| TUI | an `AtomicBool` the event loop checks before drawing |
| GTK | `async_channel` drained by `spawn_future_local` |
| wasm | an `AtomicBool` plus `requestAnimationFrame` — the flag doubles as "a frame is already scheduled" |
| egui | the `egui::Context` itself: it is `Clone + Send + Sync`, so the observer holds one and calls `request_repaint` |
| Swift / C# | a UniFFI foreign trait implemented in the shell's own language |

Coalesce: one keystroke must produce one repaint, even when it notifies twice.

**Two rendering rules.** `refresh()` renders from the core's stored scroll offset and never scrolls;
only input handlers call `follow_cursor`. Otherwise dragging a scrollbar away from the caret snaps
straight back. And `follow_cursor` must not notify when the offset does not change, or every repaint
schedules the next one forever.

## 5. Verification

The central trick: **for every boundary, build a UI-free program that exercises it, and run that
program where the UI cannot go.** Three of them exist here, and each is the same thirteen-to-
seventeen assertions:

| Boundary | Harness | Runs on |
|---|---|---|
| Rust ↔ C# | `ffi/csharp-smoke` (console app over the `.so`/`.dll`) | Linux, macOS, Windows |
| Rust ↔ Swift | `ui_mac`'s `FfiSmoke` product | Linux and macOS |
| Rust ↔ browser | `ui_web/smoke.js` — the real `.wasm` against the real page in jsdom | anywhere with node |
| A GUI's actual behaviour | `ui_egui/`'s `egui_kittest` tests — the real app, no display, no GPU | anywhere the crate builds |

Because they run before the app build in CI, a red job tells you immediately whether the bug is in
the bindings or in the XAML/SwiftUI/CSS. That distinction is worth the whole cost of writing them.

What this buys per host:

| To check | Linux host | macOS host |
|---|---|---|
| core, CLI, TUI, wasm module | native | native |
| GTK build + keymap tests | native (needs `libgtk-4-dev`, `libadwaita-1-dev`) | container (§5.1) |
| C# and Swift FFI boundaries | native, both | native, both |
| an immediate-mode GUI's *behaviour* | native, headless | native, headless |
| SwiftUI as a running app | ✗ | native |
| WinUI / XAML | ✗ | ✗ (CI on Windows only) |
| how any GUI *looks* | only on that platform | only on that platform |

The last row is the honest one, and the row above it is the interesting one. A smoke test proves
wiring, never appearance; jsdom in particular has no layout engine, so every rectangle it reports is
zero. Open the real app before claiming a UI change works.

The exception is worth understanding, because it is the one gap in this table that an architectural
choice can close. A retained-mode toolkit hides its state inside native objects that need a display
server to exist, so the most you can test off-screen is the pure functions beside it — a keymap, some
arithmetic. An **immediate-mode** GUI has no such objects: the frame is a function of the state, so a
harness can call that function, feed it synthetic events and read the accessibility tree it produces,
on any host, in milliseconds. That makes an agent able to verify a GUI change without a human looking
at a screen — which is a real reason to keep such a shell around even when it is native to nothing,
and it is why this repository has one. Two things it still does not buy: it renders no pixels, so it
says nothing about appearance, and it is that shell's behaviour it verifies, not the native shells'.
What generalises is the core underneath.

### 5.1 Linux containers with podman

On macOS, and on Linux where you want CI's exact environment, run the Linux-only work in a container.
Podman needs a Linux VM on macOS and nothing at all on Linux:

```sh
# macOS only, once:
podman machine init --now          # or: podman machine start

# Then, from the repo root, on either host:
podman run --rm -it \
  -v "$PWD":/src -w /src \
  -e CARGO_TARGET_DIR=/src/target-container \
  docker.io/library/rust:1 bash -c '
    apt-get update -qq && apt-get install -y -qq libgtk-4-dev libadwaita-1-dev
    cargo test --workspace
  '
```

Four things that will bite you otherwise:

- **`bash -c`, not `bash -lc`.** A login shell rebuilds `PATH` from `/etc/profile` and throws away
  the one the image set, so cargo and rustc vanish: `rustc: command not found`.

- **Give the container its own `CARGO_TARGET_DIR`.** Sharing `target/` between host and container
  means two toolchains fighting over the same fingerprints, and full rebuilds every time you switch.
- **On an SELinux host add `:Z` to the volume** (`-v "$PWD":/src:Z`), or the mount is unreadable.
  Do not use `:Z` on macOS; it is unnecessary there.
- **A container has no display.** It builds and tests GTK; it cannot show you the app. For that, use
  a Linux desktop — or accept that the GTK shell's testable part is its keymap, which needs no
  display anywhere.

Bind mounts on `podman machine` are slower than native disk. If a build feels wrong, that is why;
put the target directory in a container volume rather than on the mount.

### 5.2 Driving UIs without a human

- **TUI:** open a real pty and answer what the terminal is asked. Since ratatui 0.30, startup queries
  the cursor position (`ESC[6n`) and blocks until something replies — piping into `script` no longer
  works, because a pipe never answers. `ui_tui/drive.py` does this and takes the keystrokes as
  arguments.
- **Browser:** jsdom, with two caveats. Copy every DOM constructor onto `globalThis` (the generated
  glue type-checks with `instanceof Window`, `instanceof HTMLButtonElement`), and expect zeroed
  geometry, which is exactly why the metric code needs a non-zero floor.
- **An immediate-mode GUI:** its own test harness, driving the real app in-process. Two conditions
  make it work, and both are on you: the per-frame function must be callable outside the framework's
  event loop (keep the logic in a `frame(&mut Ui)` that the framework's entry point merely delegates
  to), and painted text must be given back the accessibility node that painting it costs — otherwise
  the harness, like a screen reader, finds an empty window.
- **Everything else:** the CLI. It is scriptable by design, and `--format json` exists so an agent can
  assert on `changed` and `written` instead of parsing prose.

## 6. Traps, each of which cost real time

**Core**

- Notifying observers while holding the write lock deadlocks the moment an observer reads the editor
  back — which every shell does. Drop the lock, *then* notify, and pin it with a test.
- A viewport request past the end of the document must clamp to the last line, not return nothing, or
  a stale scroll offset blanks the view.
- Convert between 0-based and 1-based addressing in exactly one place, and write down which side each
  API is on.

**Rust shells**

- Windows reports key press *and* release: act on press only, or everything types twice.
- A `Ctrl`-modified key must never also insert its character.
- Restore the terminal on *every* exit path, panic included, and restore before printing an error.
- Do not hold a `RefCell` borrow across a call that borrows it mutably — `set(x.borrow().clone())` is
  a guaranteed panic, and it will be in the handler you tested least.

**Immediate mode**

- Painting text yourself costs it its accessibility node, and a screen reader is not the only thing
  that notices: the test harness reads the same tree and finds an empty window. Claim the area as a
  widget and give it the visible text as a label.
- Input and painting happen in one function, so the ordering that is structural elsewhere becomes a
  decision: handle events, `follow_cursor`, *then* read the offset and paint.
- Never repaint on a timer. An immediate-mode shell that redraws unconditionally is polling — it
  will look correct and violate rule 4, and it should idle at zero frames a second.
- Keep the per-frame function callable outside the framework's event loop, since that is what makes
  the shell testable at all (§5.2).

**Foreign shells**

- Bindgen versions are coupled to the runtime crate and the external generators lag: read the wanted
  version out of `Cargo.lock` at build time and fail loudly on a mismatch instead of pinning it in
  prose that goes stale.
- Generated C# is `internal`: it must be compiled *into* the consuming assembly, not referenced.
- Named constructors do not become initialisers in Swift; only one called `new` does.
- Stop the platform's own undo stack (AppKit's, for one) from swallowing ⌘Z, since it knows nothing
  about your document.

**wasm**

- The observer trait is `Send + Sync` but the page is not, and a wasm module is single-threaded: hold
  a flag in the observer and touch the DOM only from the scheduled frame.
- `web-sys` is feature-gated per type. Every DOM type you touch needs a line in `Cargo.toml`.
- Guard against zero-valued measurements: before first layout every rectangle is zero, and the
  division you did not guard produces a NaN caret.

**Project**

- An MSRV that nothing checks is worse than no MSRV. Either verify it in CI on that exact toolchain,
  or drop `rust-version` and say "current stable". This repo dropped it.
- Shared CI jobs that use `--workspace` break the day a crate needs system libraries the runner lacks.
  Name the crates instead; remember the lint job *does* build everything and needs those packages.

## 7. Checklist for a new shell

- [ ] Holds no application state — only presentation concerns.
- [ ] Renders from the windowed read API, and the text widget cannot edit itself.
- [ ] Key mapping lives in a widget-free, unit-tested module.
- [ ] Repaints come from the observer, never a poll or a timer.
- [ ] `follow_cursor` is called from input handling only, never from the repaint.
- [ ] Platform conventions taken from that platform's current guidelines, not from a sibling shell.
      A portable shell is the one allowed exception, and only if a decision record says so — "it has
      no platform" is a choice to write down, not a box to leave unticked quietly.
- [ ] Any new capability landed in the core and the CLI in the same change.
- [ ] A UI-free smoke test for its boundary, running before the app build.
- [ ] Its own workflow, on its own OS runner, never cross-compiled.
- [ ] Known gaps written down rather than implied.

See `CLAUDE.md` for how each of these is resolved in this repository, shell by shell.
