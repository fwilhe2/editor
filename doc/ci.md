<!--
SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>

SPDX-License-Identifier: MIT
-->

# CI, and the one rule that shapes it

**A binary is built once per platform, and everything that has something to say about that binary
is a later step in the same job.** The tier a job belongs to is decided by its profile: debug
checks and release artifacts cannot share a compile, and everything within one of them can.

| Workflow | Tier | Does |
|---|---|---|
| `ci.yml` | checks (debug) | `reuse`, `fmt`, `clippy`, `docs`, `build` (build + every test, incl. the parity and packaging ratchets and `examples/sample.sh`), `licences` (`cargo deny`), `bootstrap` (the template renamed, built and checked for traces) |
| `shells.yml` | checks (debug) | `gtk` (test, clippy, docs — the only job with GTK installed), `win32-from-linux` and `mac-from-linux` (check, clippy and docs for the Windows and Apple targets, **on Linux**) |
| `artifacts.yml` | artifacts (release) | `linux` (three binaries → GTK frames twice → `.deb` + `.rpm`), `windows` (tests incl. `cfg(windows)` → `.exe` → import table, stack, version, icon, manifest → frames twice), `macos` on 15 and 26 (tests → both slices → frames twice → `Quill.app`, its checks, a DMG), `wasm` (bundle → jsdom smoke → upload) |

## Why the shells are checked from Linux

`cargo check`, `cargo clippy` and `cargo doc` never link. The `windows` and `objc2` crates are pure
Rust with their bindings checked in, so the Windows and macOS shells type-check for their real
targets on an Ubuntu runner, in about a minute, while the Windows and macOS runners are still
installing a toolchain. It is the same command a contributor runs on a Linux laptop:

```sh
rustup target add x86_64-pc-windows-msvc aarch64-apple-darwin x86_64-apple-darwin
cargo clippy -p quill-win32 --target x86_64-pc-windows-msvc --all-targets -- -D warnings
cargo clippy -p quill-mac --target aarch64-apple-darwin --all-targets -- -D warnings
```

`cargo build --target …` fails there, and that is not a bug: linking needs MSVC or the macOS SDK.
`ci.yml`'s `docs` job documents the shells on the host, where every `cfg(windows)` item is compiled
out — so a broken link in the window code is visible only to `shells.yml`'s target-specific `cargo
doc`. grind had six of them before it noticed.

## Things that will bite if forgotten

- **`push` is restricted to `main`** in every workflow. With `on: [push, pull_request]` each commit
  on a branch with a pull request open ran everything twice, to the same conclusion.
- **`save-if: main` on every `rust-cache`.** Caches are per branch; without it every branch kept its
  own copy and grind sat over the 10 GB limit, evicting the entries that were being hit.
- **`cargo deb` is passed `--no-build`.** It builds what it packages otherwise — one release build
  becomes four. `cli/tests/packaging.rs` reads `artifacts.yml` and fails when a packaged binary has
  no `cargo deb -p … --no-build` and `cargo generate-rpm -p …` line.
- **Every action is pinned by commit**, with the tag in a comment. A tag can be moved; a commit
  cannot.
- **`--locked` everywhere.** `Cargo.lock` is committed, and a build that quietly updates it is a
  build of something other than the commit.
- **The `wasm-bindgen` CLI version comes from `Cargo.lock`**, not from a pin in the workflow.
- **A frame is checked for reproducibility, not for appearance.** Byte-identical twice, different
  in `--dark`. Whether it *looks* right is a person's, or a screenshot's (below).
- **The macOS bundle is checked as built**: `lipo` (both slices), `plutil -lint`, `codesign --verify
  --strict`, `otool -L` (only `/System` and `/usr/lib`), `vtool` (the deployment target), and `spctl`
  *rejecting* the ad-hoc signature — a check that flips the day a Developer ID is added.
- **The Windows `.exe` is checked as built**: no DLL outside Windows in its import table (the C
  runtime is static), the 8 MB stack reserve `.cargo/config.toml` asks for, the version block, the
  icon and the manifest.

## Looking at the GUIs from Linux

- **GTK**: `gtk4-broadwayd :5`, run the shell with `GDK_BACKEND=broadway BROADWAY_DISPLAY=:5`, and
  screenshot `http://127.0.0.1:8085/` with `firefox --headless --screenshot`. Firefox shoots at the
  `load` event, before Broadway has drawn, so point it at a page that iframes Broadway and holds
  `load` open with an image from a server that sleeps a few seconds.
- **Browser**: serve `ui_web/dist` with a same-origin driver page that iframes `index.html` and
  dispatches `input` and `keydown` events into it; the same slow-image trick.
- **Win32**: `cargo xwin build -p quill-win32 --target x86_64-pc-windows-msvc`, then Wine under
  Xvfb, `xdotool` to drive it and ImageMagick's `import` to photograph it. `--render-to` works under
  plain headless Wine (`env -u DISPLAY wine … --render-to frame.bmp`).
- **macOS**: there is no way from Linux. `artifacts.yml` uploads each runner's `--render-to` frames.
