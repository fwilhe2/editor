# Decision: replace the SwiftUI shell with an AppKit/objc2 one

**Status:** not implemented, 2026-09-05. The third decision record in this repository, directly following the architectural pivot established in `decision-win32-shell.md`.

**Summary:** delete `ui_mac/` — a SwiftUI application in Swift reaching the core through UniFFI — and replace it with `ui_appkit/` (crate `editor-appkit`, binary `edit-appkit`), a Rust-direct shell that drives an `NSWindow` and draws via Core Graphics through the `objc2` and `objc2-app-kit` crates. The deliverable is a native macOS application bundle built entirely via `cargo`, requiring zero Apple developer tools beyond the standard linker.

## Context

The SwiftUI shell was a faithful macOS citizen, but maintaining it meant maintaining two parallel toolchains and languages.

Building `ui_mac/` required Xcode, the Swift compiler, and the `uniffi-bindgen` CLI. Because the Swift bindings generator is built into upstream UniFFI, the version coupling was less fragile than the C# equivalent deleted on Windows, but the fundamental friction remained: a Rust developer working on the core editor state could not touch the macOS UI without context-switching to Xcode, Swift, and Apple's build system.

## The thesis this appears to contradict

As stated in the Windows decision, abandoning a modern declarative UI framework (SwiftUI) looks like abandoning the native platform entirely.

Native does not mean using the platform's declarative UI framework. What native buys the user is the platform's behaviour: the global menu bar, Cmd+Q to quit, standard window resizing, native wheel scrolling mechanics, and integration with the system's window manager. Every one of those is available to an `NSWindow` driven by a pure C or Rust binary, and `ui_appkit/` implements every one of them by talking directly to `NSApplication` via Objective-C message passing.

What is lost is the specific look and automatic behaviors of SwiftUI, which is narrower than "nativeness".

## Decision

Add `ui_appkit/` (crate `editor-appkit`, binary `edit-appkit`) and delete `ui_mac/`.

Because this follows the deletion of the C# Windows shell, this decision forces a terminal shift in the project's architecture. **The "UniFFI foreign" row in the taxonomy is now completely empty.** Consequently, `ffi/` and the UniFFI dependency are being entirely removed from the repository. The project's founding claim of "one core, bindings into any language" is officially traded for "one core, pure Rust everywhere."

The taxonomy now reflects a unified strategy across desktop platforms:

|  | Native to a platform | Portable by design |
| --- | --- | --- |
| **Rust-direct** | `cli/`, `ui_tui/`, `ui_linux/`, `ui_win32/`, **`ui_appkit/`** | `ui_egui/` |
| **wasm-bindgen** | `ui_web/` | — |
| **UniFFI foreign** | — | — |

### Rendering: Core Graphics over TextKit

To render the editor, the shell subclasses `NSView` via `objc2::declare_class!` and overrides `drawRect:`.

Instead of routing text through Cocoa's TextKit or Core Text layout engines, the shell obtains the current `CGContext` (Quartz 2D) and paints a fixed-pitch monospace grid using exact glyph metrics. This mirrors the Win32 GDI approach exactly: by extracting the exact glyph width and line height from the font, the Rust core's caret arithmetic remains perfectly aligned with the pixels on screen.

## The thing that tips the decision

The `objc2` ecosystem **type-checks for `aarch64-apple-darwin` and `x86_64-apple-darwin` on Linux**.

Because `cargo check` does not invoke the linker, the macOS shell's source can now be type-checked, linted, and logic-tested on the Linux development machine. `appkit.yml` now validates the macOS shell in an Ubuntu CI runner in seconds, completely sidestepping the queue times and compute costs of macOS GitHub Actions runners for pull request validation.

Furthermore, `editor-appkit` adds exactly one family of dependencies to `Cargo.toml`: the `objc2` suite. The build is entirely managed by Cargo, completely eliminating `.xcodeproj` files and Swift package manifests from the repository.

## Options considered

| Option | Build requirements | Native behaviours | Verdict |
| --- | --- | --- | --- |
| **Rust + AppKit (`objc2` + Core Graphics)** | `cargo` only | yes | **chosen** |
| Keep SwiftUI / Swift / UniFFI | Xcode, Swift toolchain | yes | replaced |
| Mac Catalyst / UIKit | Xcode, Swift toolchain | partial (iPad idioms) | declined |
| NSTextView from Rust | `cargo` only | yes | **rejected on architecture** |

The final row was rejected for the same reason `EDIT` was rejected on Windows: `NSTextView` owns its own text buffer. Allowing it would create a second source of truth, violating the absolute project rule that the shell holds no editor state.

## Consequences

**Good**

* **Pure `cargo build` pipeline.** The macOS binary is compiled and linked without Xcode.
* **Linux type-checking.** The UI code is validated on Ubuntu runners, speeding up CI feedback loops.
* **Elimination of UniFFI.** The FFI boundary maintenance overhead is reduced to zero. The repository is solely Rust code.

**Bad, and accepted**

* **Complete loss of SwiftUI.** The modern polish, automatic dark mode transitions, and automatic animations of SwiftUI are gone.
* **Loss of Cocoa's built-in text behaviors.** By bypassing `NSTextView` and painting raw Core Graphics glyphs, the shell explicitly drops macOS's VoiceOver text reading, force-touch dictionary lookups, and native system spellcheck.
* **The UniFFI demonstration is dead.** The project can no longer serve as a reference architecture for cross-language FFI integration.

**Neutral**

* `ui_appkit/` now sits alongside `ui_win32/` and `ui_linux/` as a matched trio. The desktop strategy is now symmetrically pure-Rust across all three major operating systems.
