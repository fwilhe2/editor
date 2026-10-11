<!--
SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>

SPDX-License-Identifier: MIT
-->

# The method: a shared core with native shells

**Normative for this repository and for every application bootstrapped from it.** This is the
method, in the order you need it, with the rule each piece of the template exists to enforce.
The text editor here (Quill) is the worked example; [grind](https://github.com/fwilhe2/grind), an
office suite built the same way, is where most of these rules were learned — several of them the
hard way, and the hard way is quoted where it helps.

The economics are the point. A native user interface per platform used to mean paying for a
different language, toolkit and build system three or four times over, and almost nobody could
justify that. That per-platform labour is now cheap, for an agent. What stays expensive is
judgement: what the core owns, where the boundaries go, what each platform's conventions are, and
how a claim gets checked. This document is about spending effort there.

## 1. The shape

```
                         core/  — every rule, every piece of state, no UI
                                          │
      ┌───────────┬──────────┬────────────┼────────────┬────────────┬────────────┐
     cli/      ui_tui/    ui_gtk/      ui_web/     ui_win32/     ui_mac/
   scripts   a terminal   GNOME       a browser     Windows       macOS
             ratatui      GTK 4 +     wasm-bindgen  Win32 + GDI   AppKit + CoreText
                          libadwaita  + the DOM     (windows-rs)  (objc2)
```

**Every shell is Rust and depends on the core as an ordinary crate.** No FFI facade, no generated
bindings, no second language. That was not always so: this repository once had a SwiftUI shell
over UniFFI and a C#/WinUI one, and both went (`doc/decisions/`). What decided it is that both
platforms' native APIs are reachable from Rust — `windows` for Win32, `objc2` for AppKit — and
that reaching them from Rust buys the one property a foreign shell cannot have: **its source
type-checks, lints and tests on any machine**, because `cargo check` never links (§5).

A shell is a renderer and an event forwarder. It holds **no** application state. A field in a
shell that is not presentation (a status message, which dialog is open, a clipboard register in a
terminal) means the core is missing something.

## 2. The rules that carry the weight

Break any of these and the pattern quietly stops paying.

1. **One source of truth, and it is never the toolkit.** Every toolkit ships a text widget that
   owns a document — `GtkTextView`, `NSTextView`, an `EDIT` control, `contenteditable`. Do not use
   one: draw the document yourself from the core's view. The moment the toolkit holds a copy there
   are two documents, and the toolkit wins the argument. (The browser shell takes typed text through
   a hidden `<textarea>` and empties it on every keystroke — a pipe, not a store.)
2. **Reads are windowed.** `App::view(frame)` returns the rows that fit a frame; nothing hands a
   shell the whole document to render from. The moment a whole-document getter exists, three shells
   use it and the data structure underneath stops mattering. Saving is the one honest exception.
3. **Undo lives in the core**, as data: `Document::apply` returns the edit's inverse, and history is
   a stack of edits. Even *how* undo groups (a word at a time) and what "modified" means (a position
   in the history, so undoing back to the save is clean again) are core decisions — a shell that made
   them would make them differently.
4. **The core pushes, shells never poll.** `App::observe` takes an `Observer`; `App::mutate` drops
   the write lock **before** notifying, because every observer calls straight back in to re-read
   (pinned by `an_observer_may_read_back_without_deadlocking`).
5. **Whatever any window can do, the CLI can do.** A capability only a GUI has is one a script, a
   test and an agent cannot use. `cli/tests/parity.rs` makes this mechanical: it reads
   `core/src/app.rs`, and every `pub fn` in `impl App` needs a row in `doc/cli-parity.md` naming its
   command — or a `not exposed:` reason — or the build fails.
6. **Layout lives in the core; a shell supplies only metrics.** Up, Down, Home, End, Page Down, a
   click and a selection's extent are all defined by *rows on screen*, and a row is an output of
   layout. If shells wrapped text, each would own half the editing model and six would disagree
   about where Down goes. So `core/src/layout.rs` wraps (UAX #14) and moves carets (UAX #29), and a
   shell implements one trait — `Metrics`: how wide is this string, how tall is a row — in its own
   unit. Measured with the engine it draws with, or the caret lands where the ink is not.
7. **Saving never makes an existing file worse.** An untouched open-and-save returns the bytes
   that were read — BOM, CRLF, missing final newline — and what cannot be represented faithfully is
   refused (`Error::NotUtf8`) rather than mangled. Pinned by
   `every_untouched_file_comes_back_byte_for_byte`.
8. **No filesystem assumptions.** Every `*_file` method has a `*_bytes` twin; the browser has no
   paths. A load is *not* a big insert: it resets the caret and clears the history, or undo would
   resurrect text the file never had.
9. **Verbs are data.** `core/src/command.rs` lists every menu verb once — id, label, summary. Each
   shell builds its menus, palette or key table from `Command::ALL` and has a test that fails when a
   command is missing from it. A key is a platform convention and stays in the shell (Redo is Ctrl+Y
   on Windows, Ctrl+Shift+Z on GNOME, ⇧⌘Z on a Mac); the verb is the program's.
10. **Shells must diverge.** Menu bar or header bar or command palette; the dirty marker in the
    title or the close button's dot; Save/Don't Save/Cancel with Save suggested. Take each
    platform's conventions from its own guidelines, never from a sibling shell. All-alike shells mean
    paying for native and shipping cross-platform.

## 3. Order of work

Each step makes the next cheaper, and the early steps are where design mistakes are still free.

1. **The core, with tests, no UI.** The document, the edit/inverse, the history, the view and its
   frame. Write the deadlock test now.
2. **The CLI**, and the parity ratchet with it. Stateless invocations expose everything the core
   forgot to expose, and `--session` is how a stateless caller keeps a caret and a history. Decide
   0-based against 1-based here and convert in exactly one file (`cli/src/report.rs`).
3. **`examples/sample.sh`** — a document built from every capability, through the CLI only.
   `cli/tests/cli.rs` runs it, and a second test fails when a subcommand has no line in it. Every
   shell's `--render-to` and `scripts/run.sh` use its output.
4. **The terminal shell.** The cheapest real event loop: scrolling, repaint coalescing, teardown on
   every exit path. Whatever it invents that a second shell would need moves into the core *before*
   the second shell is written (this is how `layout::printable` — what a tab and a control character
   are drawn as — ended up in the core).
5. **One native desktop shell**, then the others. Keep the key mapping, the menus and the geometry in
   widget-free modules.
6. **The browser shell** — the strongest test of the boundary: a DOM is the furthest thing from
   your data structure, and it has no filesystem.

## 4. Shapes worth copying

These recur in every shell. Write them the same way each time; the repetition is the point, because
it makes the next shell a transcription rather than a design problem.

**A widget-free keymap.** Key and modifiers in, `Action` out — `Move(Motion, extend)`,
`Delete(Delete)`, `Type(char)`, `Command(Command)`. Unit-tested with no display, including "every
`Motion` has a key". On macOS the input is a *selector name* rather than a key code
(`ui_mac/src/keys.rs`), because the text system has already applied the user's key bindings.

**Menus as data.** A table of `(Command, label, accelerator)` that the window builds its menu bar,
header-bar menu or palette from, with "every command exactly once" and "no accelerator twice" as
tests. `ui_win32/src/menu.rs`, `ui_gtk/src/menu.rs`, `ui_mac/src/menu.rs`.

**A `Metrics` that measures what it draws.** Terminal cells (`unicode-width`), Pango, a canvas,
GDI's `GetTextExtentExPointW` whose advances `ExtTextOutW` is then *given*, CoreText's `CTLine`.

**An observer bridge per toolkit.** The core's `Observer` is `Send + Sync`; UI objects are not. The
bridge is always "note it anywhere, repaint on the UI thread", and always coalesces:

| Shell | Bridge |
|---|---|
| terminal | an `AtomicBool` the loop checks before drawing |
| GTK | `async_channel`, drained completely by `glib::spawn_future_local` |
| browser | an `AtomicBool` plus `requestAnimationFrame`; the flag means "a frame is scheduled" |
| Win32 | `PostMessageW(WM_APP + 1)` with the `HWND` carried as an integer; a flag coalesces |
| macOS | `performSelectorOnMainThread:` with the view carried as an address; a flag coalesces |

**Two rendering rules.** Painting renders from the core's scroll position and never scrolls; only
input handling calls `follow_caret` — or dragging a scrollbar away from the caret snaps back. And
`follow_caret` does not notify when nothing moved, or each repaint schedules the next forever.

**`--render-to`.** Every custom-drawn GUI shell can draw one frame into an image with no window and
no display: cairo on an image surface (GTK), a DIB section (Win32), a `CGBitmapContext` (macOS).
Two renders of one document must be byte-identical and `--dark` must differ; `artifacts.yml` checks
both on each platform. It is also how a drawing refactor is *proved* to be one.

**A portable half.** In the Windows and macOS shells everything that needs no window — arguments,
menus, keys, geometry, palette, PNG encoding, Info.plist — is a module outside `cfg(windows)` /
`cfg(target_os = "macos")`, so it compiles and tests on Linux.

## 5. Verification

The central trick: **for every boundary, find the check that runs where the platform cannot.**

| To check | Linux | macOS | Windows |
|---|---|---|---|
| core, CLI, terminal, wasm module | native | native | native |
| GTK build, keymap, menus, `--render-to` | native (GTK dev packages) | — | — |
| Windows shell: type-check, lint, docs, portable tests | `--target x86_64-pc-windows-msvc` | same | native |
| Windows shell linked and running | `cargo xwin` + Wine (inspection only) | — | native, CI |
| macOS shell: type-check, lint, docs, portable tests | `--target aarch64-apple-darwin` | native | same |
| macOS shell linked and running | ✗ (no Wine for AppKit) | native, CI | — |
| the browser boundary | `ui_web/smoke.sh` in jsdom | same | same |
| how any GUI *looks* | that platform, or a screenshot of it | | |

The last row is the honest one. A smoke test proves wiring, never appearance; jsdom has no layout
engine and no canvas. **Look at the real thing before claiming a UI change works.** In this
template's own construction, headless Firefox showed the browser shell measuring every row with the
canvas's 10px default font — Firefox returns an empty computed `font` shorthand — so nothing wrapped
and selections were too narrow. jsdom passed throughout. (`doc/ci.md` has how to take each
screenshot from Linux.)

### Driving a UI without a human

- **Terminal:** a real pty that answers the terminal's own queries (`ui_tui/drive.py`) — ratatui asks
  where the cursor is at start-up and blocks until something replies, so a pipe never works.
- **Browser:** jsdom with every DOM constructor copied onto `globalThis` (the glue checks
  `instanceof`), and expect zeroed geometry — which is why every metric has a non-zero floor.
- **GTK:** Broadway (`gtk4-broadwayd`) and a headless browser pointed at it.
- **Win32:** `cargo xwin build`, Wine under Xvfb, `xdotool` for keys. Run Wine headless
  (`env -u DISPLAY`) when a crash is possible, or WineDbg opens a dialog on your desktop.
- **Everything:** the CLI, which is scriptable by design; `--format json` lets an agent assert on
  `changed` and `written` instead of parsing prose.

## 6. Traps, each of which cost real time

**Core**

- Notifying observers while holding the write lock deadlocks the moment one reads back. Drop, then
  notify; pin it with a test.
- A view request past the end of the document clamps to the last line rather than returning nothing,
  or a stale scroll position blanks the screen.
- A rope's default line-break set may not be the file's: `ropey` counts a lone `\r` and five Unicode
  separators as breaks unless its default features are off.
- "Type, save, type" must be two undo entries, or the document reads clean after the second half.
  Saving seals the coalescing.

**Shells**

- Windows reports key press *and* release (crossterm too); act on press only.
- A Ctrl-modified key must never also insert its character.
- Restore the terminal on *every* exit path, panic included, and before printing the error.
- Never hold a `RefCell` borrow across a call that borrows it again — `set(x.borrow().clone())` is a
  panic, and it will be in the handler tested least. The browser shell's resize handler was written
  that way first.
- **Never hold `&mut` window state across a modal loop.** A message box, a file dialog and the Find
  dialog all run nested message loops that re-enter the window procedure; a borrow alive across one
  is a second `&mut` the moment `WM_PAINT` arrives. In `ui_win32/src/win.rs` every modal call sits
  between two short borrows, by construction.
- An empty `Vec<u16>` has a dangling pointer. Handing it to GDI was an access violation in grind
  that `cargo check`, clippy and a hundred unit tests all passed.
- `windows_subsystem = "windows"` means there is no stderr when started from Explorer — but there is
  one in CI, so print *and* show the dialog, and let an environment variable suppress the dialog.
- `MAKEINTRESOURCE(1)` is an integer smuggled in a pointer; write it as a named helper.
- An `.rc` file's `VS_VERSION_INFO` is a *string* name without `winver.h`; the version block must be
  named `1`, or Explorer finds nothing.
- A macOS window created from Rust must have `setReleasedWhenClosed(false)`.
- The macOS deployment target is a claim: `vtool` reads it back off both slices.

**Browser**

- `web-sys` is feature-gated per type; every DOM type touched needs a line in `Cargo.toml`.
- The `wasm-bindgen` CLI must equal the crate exactly. `ui_web/build.sh` reads the version from
  `Cargo.lock` and refuses a mismatch rather than producing glue that fails at load time.
- Turn kerning and ligatures off for the document's CSS if a shell measures per character.

**Project**

- An MSRV nothing checks is worse than none. This template has none: current stable.
- CI jobs using `--workspace` break the day a crate needs system libraries; name the crates.
- See `doc/ci.md` for the CI traps (cache budget, `cargo deb --no-build`, duplicate runs).

## 7. Checklists

**Adding a capability**

- [ ] The method on `App`, with tests in `core/`.
- [ ] Its CLI subcommand in the same change, and its row in `doc/cli-parity.md`.
- [ ] A line in `examples/sample.sh` that uses it.
- [ ] If it is a menu verb: a variant in `Command`, and then every shell's menu table (the shells'
      tests will tell you which ones you missed).
- [ ] If it is a key: every shell's keymap, each with its own platform's key.
- [ ] Each shell's doc (`doc/shells/*.md`): built, or a named gap.

**Adding a shell**

- [ ] Holds no application state — only presentation.
- [ ] Draws from `App::view`; no toolkit text widget owns the text.
- [ ] A `Metrics` measured with the engine that draws.
- [ ] Keymap and menus in widget-free modules, tested — every `Command`, every `Motion`.
- [ ] Repaints come from the observer, coalesced; `follow_caret` from input handling only.
- [ ] The platform's own conventions, from its own guidelines.
- [ ] `--render-to`, if it draws its own pixels, and a reproducibility check in `artifacts.yml`.
- [ ] Checks in `shells.yml` or `ci.yml`, a release build in `artifacts.yml`, packaging, and a row in
      `cli/tests/packaging.rs`.
- [ ] `doc/shells/<name>.md`: conventions, how it is verified, and its gaps written down.
