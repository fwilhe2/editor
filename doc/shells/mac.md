<!--
SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>

SPDX-License-Identifier: MIT
-->

# The macOS shell — `ui_mac/`, binary `quill-mac`

AppKit, CoreGraphics and CoreText from Rust, through `objc2` — no Swift, no Xcode project.
`doc/decisions/0002-appkit-shell.md` is why.

## The decision that shapes it

**AppKit draws the chrome; the shell draws only the document.** The menu bar, the window, the
alerts and the open and save panels are AppKit's own, so they look like every Mac application's;
one flipped `NSView` paints the document with CoreText, measured by the same engine (a `CTLine` is
measured and the same `CTLine` drawn).

## Conventions

An application menu (About, Hide, Quit), File, Edit, Window. The Edit menu sends the **standard
selectors** — `undo:`, `cut:`, `selectAll:` — down the responder chain, so ⌘C works in an
alert's text field too; the view answers `undo:` from the core's history, never AppKit's undo
manager. Keys go to `interpretKeyEvents:` and come back as selectors (`moveWordLeft:`), so the
user's own key bindings apply. Unsaved changes are the dot in the close button
(`setDocumentEdited`), and before work is lost: "Do you want to save the changes made to the
document …?" with Save, Don't Save, Cancel. Closing the window quits (one document per process).
The bundle *offers* to open plain text (`LSHandlerRank` `Alternate`) and never takes the default.

## Structure

| File | Holds |
|---|---|
| `args.rs` | `[FILE] [--render-to PNG] [--size WxH] [--dark]`, `--info-plist`, and AppKit's own `-Key value` arguments let through |
| `menu.rs` | the menu bar as data, with the standard selectors and `quillCommand:` + a tag for the rest |
| `keys.rs` | text-system selectors to actions |
| `theme.rs`, `png.rs`, `plist.rs` | palette and geometry; a dependency-free PNG encoder; the Info.plist |
| `metrics.rs` | `fold` (portable) and `CoreText`, the `Metrics` |
| `paint.rs` | drawing into a flipped `CGContext` — the view's, or `--render-to`'s bitmap |
| `view.rs` | the `NSView` subclass: drawing, keys, `NSTextInputClient`, the verbs, the dialogs |
| `app.rs` | `NSApplication`, the delegate, the window, the menu bar |

The icon is `data/icon.png` (1024 px, made from the GTK shell's SVG with `magick -background none
… -resize 1024x1024`); `.github/scripts/mac-bundle.sh` turns it into an `.icns` on the runner with
`sips` and `iconutil`.

## Verification

From Linux: `cargo test -p quill-mac` (menus, selectors, PNG, plist, the advance fold), and
`check`/`clippy`/`doc` for `aarch64-apple-darwin` and `x86_64-apple-darwin`. **It cannot be linked
or run without a Mac.** On `macos-15` and `macos-26`: the tests, both slices in release, two renders
byte-identical with `--dark` different, and the bundle — universal, `plutil`, `codesign`, `otool`,
`vtool`, `spctl` rejecting the ad-hoc signature — and a DMG.

## Named gaps

- **Not yet run on a Mac by a person.** CI links it and renders frames; nobody has typed into the
  window. Treat the first interactive run as a test.
- No inline input-method composition (`hasMarkedText` is always false); grind's Mac shell holds
  marked text and is the model for adding it.
- No `NSScrollView`: the view scrolls through the core, so no overlay scroller appears.
- Find is an alert with a text field, not a find bar.
- Ad-hoc signed only; notarisation needs a Developer ID.
