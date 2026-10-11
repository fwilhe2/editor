<!--
SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>

SPDX-License-Identifier: MIT
-->

# Decision: make the repository a template, in grind's image

**Status:** accepted and implemented, 2026-10-11.

## Context

This repository pioneered the shared-core, native-shell method with a deliberately tiny editor and
seven front-ends — including a SwiftUI shell over UniFFI and a portable egui shell. Since then
[grind](https://github.com/fwilhe2/grind), an office suite, was built the same way at a much larger
scale, and learned what the method needs to hold up: layout in the core, verbs as data, ratchets
that read the source, frames that can be compared, CI in two tiers, and native shells for Windows
and macOS written in Rust. The editor had fallen behind the method it introduced.

## Decision

Rebuild the editor as a **template**: a small but non-trivial application that carries every
practice grind settled on, plus the instructions and the script to turn it into another application.

1. **Every shell is Rust.** The SwiftUI shell, its UniFFI facade (`ffi/`) and the C# history go;
   macOS is AppKit through `objc2` (the plan in [0002](0002-appkit-shell.md), now built) and
   Windows stays Win32 through `windows` ([0001](0001-win32-shell.md)). One language, one build
   system, and every shell's source checkable from any machine.
2. **The egui shell goes.** It was the one GUI whose behaviour was tested headlessly, and its value
   was real — but it is native to nothing, which is the opposite of what a template for this method
   should teach. Its role is taken by narrower, native means: widget-free modules tested everywhere,
   `--render-to` frames compared byte for byte, and screenshots of the real shells.
3. **The core grows to what grind's needed**: line layout behind a `Metrics` trait, a view of only
   what fits a frame, selection, word-wise undo coalescing, "dirty" as a position in history, byte-
   for-byte round trips of BOM and CRLF, find, and a table of every menu verb.
4. **The example application is called Quill.** The repository stays `editor`, and Quill is a
   plain text editor — but its identity tokens (`quill`, `Quill`, `QUILL`, the application ID)
   appear nowhere as ordinary prose, so `scripts/bootstrap.py` can rename them with a substitution
   and `--check` can prove nothing was left. With "editor" as the token, every comment calling the
   program an editor would have been rewritten too.

## Consequences

- One binding mechanism fewer to explain, and the "any language" claim the UniFFI shells stood for
  is gone. A project that needs a foreign shell (Kotlin on Android, say) brings its own facade crate;
  the rule that the core's API is shaped for Rust and the facade forwards one-to-one still applies.
- The macOS shell is developed without a Mac: type-checked from Linux, linked and run only in CI.
  Its first interactive run is still owed.
- The repository is larger, and every addition is held by a test or a CI step, which is the point.
