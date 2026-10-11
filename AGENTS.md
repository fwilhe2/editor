<!--
SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>

SPDX-License-Identifier: MIT
-->

# AGENTS.md

Instructions for coding agents working in this repository. Kept short on purpose: the reasons
behind a piece of code live in its own doc comments and in `doc/`, so read those when you touch an
area rather than expecting them here.

## What this is

A **template** for applications built as one Rust core with native shells, and a worked example of
it: **Quill**, a plain text editor. Six front-ends, all Rust, none holding document state —
`quill` (CLI), `quill-tui`, `quill-gtk` (GNOME), `quill-web` (wasm), `quill-win32` (Win32 + GDI),
`quill-mac` (AppKit + CoreText via objc2).

**If you were asked to start a new application from this template, stop here and follow
[`doc/bootstrap.md`](doc/bootstrap.md)** (Claude Code: the `bootstrap` skill).

| Read | For |
|---|---|
| `doc/architecture.md` | **The method — normative.** The ten rules, the order of work, the shapes every shell repeats, verification, the traps, and the checklists for a new capability and a new shell |
| `doc/bootstrap.md` | Turning the template into another application |
| `doc/ci.md` | The two CI tiers, why the Windows and macOS shells are checked from Linux, the CI traps, and how to screenshot each GUI from Linux |
| `doc/cli-parity.md` | Every `App` method and the CLI command reaching it — checked by a test |
| `doc/shells/*.md` | Each shell's conventions, how it is verified, and its named gaps |
| `doc/decisions/` | Why Win32, why AppKit from Rust, why the template is shaped as it is |

## The rules that are checked by machine

Breaking one fails the build; know which test will tell you.

1. **Every `pub fn` in `impl App` has a CLI command** or a `not exposed:` reason in
   `doc/cli-parity.md` (`cli/tests/parity.rs`).
2. **Every CLI subcommand is used in `examples/sample.sh`**, and the script runs
   (`cli/tests/cli.rs`).
3. **Every `Command` is in every shell's menus/keys** (each shell's `menu.rs`/`keymap.rs` tests), and
   every `Motion` has a key in every shell.
4. **Every workspace member is packaged or says why not**, and every packaged binary has a `.deb`, an
   `.rpm` and its lines in `artifacts.yml` (`cli/tests/packaging.rs`).
5. **An untouched save returns the bytes read** (`core/src/encoding.rs`, `app.rs` tests).
6. **The observer is called after the lock is dropped** (`an_observer_may_read_back_without_deadlocking`).
7. **Formatting, clippy with `-D warnings`, rustdoc with `-D warnings`, REUSE, and `cargo deny`** —
   all CI gates.
8. **The template renames cleanly** (`ci.yml`'s `bootstrap` job).

## The rules that are not, and matter as much

- **No shell holds document state, and no toolkit text widget holds the text.** Draw from
  `App::view`; forward input to the core.
- **A new capability lands in the core and the CLI in the same change**, then the shells.
- **Layout questions are the core's**; a shell answers only `Metrics` — and measures with the same
  engine it draws with.
- **Painting never scrolls**; input handling calls `follow_caret`.
- **Each shell follows its own platform's conventions**, from that platform's guidelines.
- **Never hold a borrow across a modal loop** (Win32 dialogs re-enter the window procedure) or a
  `RefCell` borrow across a call that borrows it again.
- **Positions are 0-based in the core**; only `cli/src/report.rs` converts to 1-based.
- **Look at a GUI change** (`scripts/run.sh`, or `doc/ci.md`'s screenshot recipes) before saying it
  works. Tests prove wiring and reproducibility, not appearance. Say plainly what was not run — the
  macOS shell cannot be linked or run off a Mac.

## Commands

```sh
cargo test                                  # every crate that needs no system package
cargo test -p quill-core                    # one crate
cargo test -p quill-cli --test parity       # one test file
scripts/check.sh                            # every gate CI has that this machine can run
cargo fmt --all
cargo run -p quill-cli -- --help
scripts/run.sh tui|gtk|web|win32            # build a shell and open the sample document in it

# GTK: needs libgtk-4-dev + libadwaita-1-dev; not in the default build
cargo test -p quill-gtk
cargo run -p quill-gtk -- notes.txt --render-to /tmp/frame.png   # one frame, no display

# Windows and macOS, checked from any host (checking never links)
cargo clippy -p quill-win32 --target x86_64-pc-windows-msvc --all-targets -- -D warnings
cargo clippy -p quill-mac --target aarch64-apple-darwin --all-targets -- -D warnings
cargo xwin build -p quill-win32 --target x86_64-pc-windows-msvc  # links, for Wine; never shipped

# Browser
ui_web/build.sh debug && ui_web/smoke.sh debug
```

`cargo build --target <windows or apple>` **fails** off those platforms, and that is not a bug.

## Conventions

- `ponytail:` comments mark a deliberate shortcut with a known ceiling and the way past it. Read the
  reason before "fixing" one.
- Comments say *why*, in full sentences; the doc comment on a module is its design note.
- Every new file gets an SPDX header (`reuse annotate --copyright "…" --license MIT path`), or an
  entry in `REUSE.toml` if it cannot carry one.
- Dependencies are declared once, in the workspace `Cargo.toml`, and each comes with a comment
  saying why it is there. `--locked` in CI; there is no MSRV.
- GitHub Actions are pinned by commit hash with the tag in a comment.
- Commit or push only when asked.
