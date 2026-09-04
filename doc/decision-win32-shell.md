# Decision: replace the WinUI shell with a Win32 one

**Status:** accepted and implemented, 2026-09-04. The second decision record in this repository,
following the shape of [`decision-egui-shell.md`](decision-egui-shell.md). Unlike that one it has no
companion plan document: the shell was small enough to build in a single change, and writing a
staged plan after the fact would be a fiction. If a future shell is large enough to need stages, the
egui plan is still the template.

**Summary:** delete `ui_windows/` — a WinUI 3 application in C# reaching the core through UniFFI —
and replace it with `ui_win32/` (crate `editor-win32`, binary `edit-win32`), a Rust-direct shell
that draws a plain Win32 window with GDI through Microsoft's own [`windows`](https://crates.io/crates/windows)
crate. The deliverable is an executable that **depends on nothing Windows does not already ship**.

## Context

The WinUI shell worked, and its CI had been green on every run since it was written. The problem was
never that it broke; it was what it took to have at all.

Building it needed four separate installations, which is why
`scripts/install-windows-dependencies.ps1` existed as a 174-line PowerShell script:

| Prerequisite | Why |
|---|---|
| Visual Studio Build Tools + Windows SDK | MSVC linker, and the Windows App SDK's build targets |
| .NET 8 SDK | the shell is C# |
| rustup, MSVC toolchain | the core |
| `uniffi-bindgen-cs` from a git tag | the C# bindings, generated not committed |

And *running* it needed two more things that are not part of Windows: the .NET 8 runtime and the
Windows App Runtime. An unpackaged WinUI 3 application on a clean Windows install does not start.

Three costs fell out of that, and only the first is obvious:

- **A 403-line shell cost 186 lines of installer.** The XAML and its code-behind were not the weight;
  the toolchain was.
- **The `uniffi` ↔ `uniffi-bindgen-cs` pin was the most fragile thing in the repository.** Two
  versions coupled by hand, one of them from a third party (NordSecurity) that necessarily lags
  upstream UniFFI, which meant uniffi's latest release was usually *not* the one to take. Swift has
  never had this problem, because its generator is built from `ffi/` itself.
- **Nothing about it could be examined from the development machine.** Not built, not type-checked,
  not linted. `ffi/csharp-smoke` tested the *boundary* on Linux, which is real and was the right
  design, but the shell above it was unreachable.

## The thesis this appears to contradict

The README argues that native UIs are worth paying for and that agents made paying for them
affordable. `ui_windows/` was the strongest evidence for that claim on Windows: Fluent controls, a
Mica backdrop, a `CommandBar`, a `ContentDialog`. Replacing it with a hand-drawn GDI window looks
like giving that up to save a few megabytes of SDK.

Two things make it a different trade than it looks:

**Following a platform's conventions and using a platform's controls are not the same thing.** The
GNOME shell uses libadwaita's widgets; the macOS shell uses SwiftUI's. But what "native" buys the
*user* is the platform's behaviour — the shell font at the right size, the user's own wheel-scroll
and caret-blink settings, Ctrl+Y for redo, a Save/Don't Save/Cancel dialog with Save as the default,
a title bar that goes dark when the theme does, per-monitor DPI that reflows on the drag between
monitors. Every one of those is available to a Win32 window, and `ui_win32/` does every one of them.
What is genuinely lost is the *look* of Fluent, which is narrower than "nativeness".

**This is still a native shell, not a second portable one.** The distinction `decision-egui-shell.md`
draws is that `ui_egui/` deliberately looks the same everywhere and belongs to nowhere. `ui_win32/`
belongs to exactly one platform, follows that platform's guidelines, and will not run anywhere else.
The taxonomy row does not move; only the toolkit inside it does.

What must not be quietly reframed is the loss on the other side, which is real and is the strongest
argument against this decision — see *Consequences*.

## Decision

Add `ui_win32/` (crate `editor-win32`, binary `edit-win32`) and delete `ui_windows/` and
`ffi/csharp-smoke/`. `ffi/` itself stays: macOS still needs it, and it is still where the UniFFI
annotations live.

This **moves Windows from one class of shell to another**, which the taxonomy has to record:

|  | Native to a platform | Portable by design |
|---|---|---|
| **Rust-direct** | `cli/`, `ui_tui/`, `ui_linux/`, **`ui_win32/`**, planned `ui_qt/` | `ui_egui/` |
| **wasm-bindgen** | `ui_web/` | — |
| **UniFFI foreign** | `ui_mac/` | — |

Everything else about it is ordinary: it holds no editor state, renders from `get_viewport`,
repaints from the observer, and adds no capability the CLI does not already have.

### Why GDI, and not Direct2D or DirectWrite

Both are equally shipped with Windows, so this is not a dependency question. It is a question of how
much code the job needs.

The entire surface this shell requires from a text stack is two numbers — the width of a character
and the height of a line — which is what `layout::Metrics` has been in `ui_web/` and `ui_egui/` all
along. `GetTextMetricsW` on a fixed-pitch font returns exactly those two as `tmAveCharWidth` and
`tmHeight + tmExternalLeading`. A Direct2D/DirectWrite pipeline would mean a device, a swap chain, a
render target, a text format, device-loss handling and a resize path, to arrive at the same two
numbers and the same monospace grid.

`ExtTextOutW` with an explicit advance per glyph is what makes the grid exact rather than
approximately exact: the caret arithmetic assumes every character is one cell wide, and passing the
advance array forces the font to agree, whatever it would have done on its own.

DirectWrite is the right upgrade if this shell ever wants proportional fonts, ligatures or complex
script shaping. It wants none of those.

### Why not WinUI 3 from Rust

Two routes exist and both were rejected, but they are worth recording because they are the ones a
reader will ask about.

**[`windows-reactor`](https://github.com/microsoft/windows-rs/issues/4479)** is Microsoft's own
React-shaped declarative UI library over WinUI 3, in pure Rust, announced in the May 2026 Rust for
Windows update. It is the only route that would keep Fluent *and* drop C#. Declined on three counts:
it is version 0.100.0 published on 2026-09-03 with 376 total downloads and two releases ever; it
still requires the Windows App SDK 2.0.1+ runtime on the machine, so the *runtime* dependency this
decision exists to remove would remain and only the build-time .NET half would go; and its hooks
(`use_state`, `use_effect`) put state in the shell, which is the one thing every shell here is
forbidden to do. Trading a documented version pin for a four-month-old 0.x framework is not a
reduction in fragility. **Revisit in a year.**

**[`winio-winui3`](https://crates.io/crates/winio-winui3)** (0.4.5) is a community subset of WinUI 3
bindings. Same runtime dependency, less backing, no.

## The thing that tips the decision

The `windows` crate **type-checks for `x86_64-pc-windows-msvc` on Linux**, because `cargo check` does
not link and therefore never needs MSVC. Verified on the development machine before this was written:

```
$ rustup target add x86_64-pc-windows-msvc
$ cargo check -p editor-win32 --target x86_64-pc-windows-msvc
    Checking editor-win32 v0.1.0
    Finished `dev` profile in 0.34s
```

That is a capability `ui_windows/` never had in any form. The Windows shell's source can now be
type-checked, linted and — for `keymap.rs` and `layout.rs` — unit-tested on the machine this
repository is developed on, and in CI on an Ubuntu runner in about a minute. `win32.yml` has a job
that does exactly this, alongside the real Windows build.

It does not prove the window works. What does — further than expected — is Wine, which turned out to
run this shell well enough to drive and photograph; see *Running it on Linux* below.

The second surprise is the dependency count. `editor-win32` adds **exactly one entry to
`Cargo.lock`** — itself. The `windows` crate at 0.62.2 was already in the lock file, pulled in by
`eframe → egui-winit → accesskit_winit → accesskit_windows`, so the Windows shell's entire
third-party dependency footprint was already paid for by the portable one.

## Running it on Linux, and why that is not a build path

`cargo-xwin` links the real `x86_64-pc-windows-msvc` binary on Linux — it uses `lld-link` against
Microsoft's own CRT and SDK, downloaded and licence-accepted by `xwin` — and Wine runs it. On a
headless machine, Xvfb gives it a display and a screenshot can be taken of the result. On Fedora:

```sh
sudo dnf install clang lld wine xorg-x11-server-Xvfb ImageMagick
cargo install cargo-xwin

cargo xwin build -p editor-win32 --release --target x86_64-pc-windows-msvc

Xvfb :99 -screen 0 1200x800x24 &
export DISPLAY=:99 WINEDLLOVERRIDES="mscoree,mshtml=" WINEDEBUG=-all
wine target/x86_64-pc-windows-msvc/release/edit-win32.exe somefile.txt &
import -window root /tmp/shot.png
```

**This is an inspection aid, exactly like `cargo check --target`, and Rule 9 still stands.** The
artifact that ships comes off the `windows-latest` runner in `win32.yml` and nowhere else. Nothing
here is in CI and nothing here should be: a green Wine run is not evidence that Windows is happy,
and adding it would quietly turn a debugging convenience into a release path.

It earns its place because of what it caught. Driving the window through XTEST found a **soundness
bug that reading the code did not**: `MessageBoxW` runs its own message loop, so the window is
repainted *while* the close handler holds a `&mut App`, and the re-entered window procedure produced
a second `&mut App` to the same object. `on_close` is now a free function that borrows twice, briefly,
either side of the dialog, and never across it. Any future modal — a file picker, a find bar — has
to follow that shape.

What Wine does **not** cover, and what still needs a real Windows machine:

- `DwmSetWindowAttribute` is largely inert, so the dark title bar is unverified.
- The theme registry key does not exist in a fresh prefix, so the shell takes its documented
  light-mode fallback and **dark mode is untested end to end**.
- Consolas is absent, so every screenshot exercises the `FIXED_PITCH | FF_MODERN` substitution path
  rather than the intended font. Useful — that fallback is now known to work — but it is not what a
  Windows user sees.
- Per-monitor DPI, `WM_DPICHANGED`, and how any of it actually looks under Windows 11's compositor.

## Options considered

| Option | Runtime deps beyond Windows | Fluent look | Checkable on Linux | Verdict |
|---|---|---|---|---|
| **Win32 + GDI, `windows` crate** | none | no | type-check + logic tests | **chosen** |
| Win32 + Direct2D/DirectWrite | none | no | same | declined — same result, much more code |
| Keep WinUI 3 / C# | .NET 8 runtime, Windows App Runtime | yes | boundary only | replaced |
| `windows-reactor` (WinUI 3 in Rust) | Windows App Runtime | yes | unknown | declined, revisit |
| `winio-winui3` | Windows App Runtime | partial | unknown | declined |
| System XAML Islands | none | dated Fluent | no | declined — deprecated, and fussy unpackaged |
| A read-only `EDIT` control | none | partial | no | **rejected on architecture** |

The last row is worth stating explicitly because it looks like the cheap option: an `EDIT` or
rich-edit control owns its own text buffer, which would make it a second source of truth exactly as
`GtkTextView`, WinUI's `TextBox` and `contenteditable` would. It is the one thing this project
refuses in every shell, and being easier is not an argument.

**System XAML Islands** (`Windows.UI.Xaml.Hosting`, shipped with Windows 10 1903+ rather than with
the App SDK) is the only route to real XAML controls with no runtime install. It is deprecated in
favour of WinUI 3 islands, awkward in an unpackaged process, and would drag the WinRT async model
into a shell that has no async work.

## Consequences

**Good**

- **The executable runs on a clean Windows install.** No .NET, no Windows App SDK, no Visual C++
  redistributable — `.cargo/config.toml` links the MSVC CRT statically, which is what removes the
  last of those. `win32.yml` reads the import table back and fails if anything outside Windows
  appears, so the claim is checked rather than asserted.
- **Two prerequisites instead of four**, and the 174-line PowerShell installer is unnecessary:
  rustup with the MSVC toolchain, and VS Build Tools for the linker.
- **The worst version coupling in the repository is gone.** `uniffi-bindgen-cs` and its hand-matched
  tag are no longer anywhere. Two remain — `wasm-bindgen`/its CLI, and `eframe`/`egui_kittest` — and
  both are better behaved.
- **The Windows shell is finally inspectable from Linux**, which is new for a native shell here.
- One more Rust-direct shell, so the core's contract is exercised from a fifth direction: a raw
  message loop with no toolkit above it at all.

**Bad, and accepted**

- **This costs the project its only C# shell, and with it half of the UniFFI demonstration.** This is
  the real price and it should not be softened. `ffi/` remains and macOS still exercises it, but the
  claim "one core, bindings into any language" now rests on one language rather than two — and on
  the *easier* one, since Swift's generator ships with uniffi while C#'s is third-party and lags.
  The architecture guide's foreign-shell table is now a table with one row of evidence behind it.
  If the parity of the *architecture* matters more than the parity of the *binary*, this decision is
  wrong.
- **`ffi/csharp-smoke` is gone**, and it was one of only two FFI boundary checks runnable on Linux.
  `FfiSmoke` (Swift) survives and covers the same 13 checks, so the boundary is still verifiable
  here — but by one harness instead of two.
- **No Mica, and this was misjudged when the option was first argued.** `DWMWA_SYSTEMBACKDROP_TYPE`
  is reachable and shipped, but a Mica backdrop only shows through pixels the application does not
  paint, and a GDI window that fills its client area with an opaque brush covers all of them.
  Mica and opaque GDI text rendering are not compatible. `DWMWA_USE_IMMERSIVE_DARK_MODE` — the dark
  title bar — *is* compatible and is implemented; that is the part that survives.
- **No Fluent controls.** The WinUI `CommandBar` is replaced by a **menu bar** — File (Save, Exit)
  and Edit (Undo, Redo), built with `CreateMenu`/`AppendMenuW`, greyed per `can_undo`/`can_redo`/
  `is_dirty` at `WM_INITMENUPOPUP`. That is the Windows convention for an editor this size and it
  costs no dependency, but it is a plainer thing than a `CommandBar` with icons. A *toolbar* would
  mean the Common Controls v6 toolbar class, which needs the manifest this binary deliberately does
  not have. The close confirmation is a `MessageBoxW` rather than a `ContentDialog` — the same
  three-button conversation, drawn by the operating system, in an older idiom.
- **No accessibility beyond the caret.** WinUI gave a full UI Automation tree for free. A painted
  GDI window gives none; the system caret at least reports its position to assistive technology and
  to IMEs, which is why the caret is a real `CreateCaret` caret rather than a painted rectangle.
  `accesskit_windows` is the fix if this ever matters, and it is already in the lock file.
- **Unsafe code, for the first time in a shell here.** A window procedure that stores a `Box` pointer
  in `GWLP_USERDATA` is the standard Win32 arrangement and it is still `unsafe` in a repository that
  had none. It has already produced one real aliasing bug, in the close dialog, caught by running the
  thing rather than by reading it — the cost is not hypothetical.
- **It has still not been run on Windows.** It has been run under Wine — typing, arrows, undo, redo,
  save to disk, wheel scrolling, the close dialog and all three of its answers — and photographed.
  That is a great deal more than "type-checked", and it is not the same as Windows. The list of what
  Wine cannot speak for is above.

**Neutral**

- Tabs are drawn as a single space so the character grid matches the caret arithmetic; the document
  keeps its tab. A character outside the basic multilingual plane takes two cells rather than one,
  for the same reason — the same class of gap as the CRLF handling in `EditorState::backspace`.
- `ui_linux/` is now the only shell whose directory name misleads, and more so than before: with
  `ui_win32/` next to it, `ui_windows/` is gone but `ui_linux/` is still GTK-specific. Renaming it to
  `ui_gtk/` is now overdue rather than merely tidy.

## What this does not change

- **Feature parity.** This shell exposes exactly what existed: open a file from `argv`, edit, move,
  click, scroll, undo/redo, save. It needs no new core capability and therefore no new CLI
  subcommand — the same outcome `ui_egui/` had, and what the parity rule predicts for a shell that
  changes toolkit rather than platform capability.
- **Rule 9.** Artifacts are still built per OS: `edit-win32.exe` comes off a `windows-latest` runner.
  The Linux job type-checks and never links, which is not cross-compilation and must not become it.
- **`ffi/` and `ui_mac/`.** The UniFFI facade, its Swift bindings and `FfiSmoke` are untouched.
- **The core.** No API is added, removed or modified by this decision.

## When to revisit

- **If `windows-reactor` reaches 1.0 and the Windows App SDK becomes part of Windows**, the Fluent
  look becomes available at this decision's price and this should be reconsidered on the spot.
- **If losing the C# shell turns out to have mattered** — if the architecture guide's foreign-shell
  claims start reading as unsupported — the answer is to add a foreign shell back somewhere else
  (Kotlin is the obvious candidate and needs no Windows) rather than to undo this.
- If nobody ever runs `edit-win32.exe` on a clean Windows install, the central claim is untested and
  the import-table check in CI is doing all the work.

## Evidence behind this document

Everything asserted above was checked on 2026-09-04 rather than recalled:

| Claim | How it was checked |
|---|---|
| `windows` 0.62.2 type-checks for `x86_64-pc-windows-msvc` on Linux | `cargo check --target`, exit 0, on this machine |
| the whole shell type-checks and lints clean for Windows | `cargo clippy -p editor-win32 --target x86_64-pc-windows-msvc --all-targets -- -D warnings` |
| `editor-win32` adds exactly one entry to `Cargo.lock` | `git diff Cargo.lock`; 543 packages → 544 |
| `windows` 0.62.2 was already in the lock file | `cargo tree -i windows@0.62.2`: `accesskit_windows ← … ← eframe ← editor-egui` |
| the 23 windowless tests run on Linux | `cargo test -p editor-win32` on the host |
| `+crt-static` is supported on windows-msvc and the default is dynamic | the Rust reference, *Static and dynamic C runtimes* |
| the WinUI shell needed four build prerequisites | the deleted `scripts/install-windows-dependencies.ps1` |
| `windows-reactor` is 0.100.0, published 2026-09-03, 376 downloads, 2 releases | the crates.io API |
| `winio-winui3` is 0.4.5 | the crates.io API |
| `windows-reactor` requires the Windows App SDK 2.0.1+ runtime | its own prerequisites, in the May 2026 update |
| the `windows` crate covers Win32, COM and WinRT but not WinUI 3 XAML | Microsoft's *Rust for Windows* page |
| the linked `.exe` imports only OS DLLs | `objdump -p`: user32, gdi32, dwmapi, kernel32, advapi32, oleaut32, ntdll, one api-set — no vcruntime, no ucrtbase |
| `+crt-static` survives a `cargo-xwin` build | the link pulled `libcmt.lib`/`libvcruntime.lib`, the static CRT |
| the shell runs, edits, scrolls, saves and closes | driven under Wine 11 on Xvfb via XTEST, screenshotted at each step |
| `follow_cursor` is not called from painting | wheel-scrolled away from the caret at Ln 61; the view stayed put, and a keystroke brought it back |
| the wheel honours `SPI_GETWHEELSCROLLLINES` | 5 notches moved the view 15 lines, the Windows default of 3 per notch |
| the close dialog re-enters the window procedure | the aliasing bug it caused, and the fix, are described above |
| the menu works and greys correctly | Undo/Redo both grey on open, Undo enabled after typing, Undo clicked from the menu undid one character |

One claim in here is **reasoned, not measured**, and is flagged where it appears: that Mica cannot
show through opaque GDI painting. Dark mode and the dark title bar are **untested**, for the reasons
listed under *Running it on Linux*. All three need a Windows machine.
