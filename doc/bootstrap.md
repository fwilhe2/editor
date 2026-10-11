<!--
SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>

SPDX-License-Identifier: MIT
-->

# Bootstrapping an application from this template

**Instructions for an agent** (and for a person following along). This repository is a template:
a plain text editor called **Quill**, built as one Rust core with six front-ends, and with every
check the method relies on already wired. Bootstrapping turns it into *your* application — a
different document, different verbs — while keeping the method, the shells, the ratchets and CI.

`scripts/bootstrap.py` does the mechanical half. Everything from step 4 on is judgement, in an
order that keeps the build green at every step. Read `doc/architecture.md` first: it is the method,
and this file assumes it.

## 0. Ask before you start

These are the person's to decide, not yours. Ask, in one message, and wait:

1. **The name.** A slug (`notes`, `ledger`, `map-maker`) for crates and binaries, and the one-word
   name a person reads (`Notes`). A display name with spaces is a follow-up edit (step 8).
2. **The application ID**, reverse DNS: `io.github.<user>.<Name>` for a GitHub-hosted project.
3. **The repository URL**, if it differs from this one.
4. **The licence, and the copyright holder.** The template is MIT and every file carries an SPDX
   header naming its author. Do not guess either.
5. **The domain**: what the document is, what a person does to it, and what "the same file back"
   means for it (rule 7 of the method). Get one paragraph, and the five verbs that matter most.
6. **Which shells.** Default: all six. Dropping one is cheap later (step 7); adding one back is not.

## 1. Rename

```sh
scripts/bootstrap.py notes --name Notes --app-id io.github.someone.Notes \
    --repo https://github.com/someone/notes
scripts/bootstrap.py --check quill     # must print: no trace of 'quill'
```

It rewrites every tracked text file (Cargo.lock included) and renames the files named for the
application ID. It leaves three files alone because they describe the template — this one, the
script itself, and `.claude/skills/bootstrap/SKILL.md`. Delete all three once you are done (step 9).

## 2. Licence and authorship

If the licence changes from MIT:

- `Cargo.toml`'s `[workspace.package] license`, `LICENSE`, and `LICENSES/<id>.txt` (download the
  SPDX text; remove `LICENSES/MIT.txt` if nothing is MIT any more).
- Every SPDX licence line in every file header (they are uniform, so one `grep -rl` and one `sed`), and `REUSE.toml`.
- `deny.toml`'s allow-list is about *dependencies* and does not change with your own licence — but a
  copyleft licence of your own may let you accept more; that is a decision to write down there.
- The metainfo file's `project_license`, and `ui_win32/data/app.rc`'s `LegalCopyright`.

If the author changes, rewrite every `SPDX-FileCopyrightText` line (they are uniform; one `sed`).
Then `reuse lint` (or `scripts/check.sh`, which runs it in a container) must pass.

## 3. Prove the rename

```sh
scripts/check.sh        # every gate this machine can run; it says which it skipped
```

Do not go further until this is green. Commit here: "Bootstrap from the Quill template".

## 4. Replace the domain — core first

The text editor is the example; your document is not text. Work **inside `core/` only** until its
tests pass, keeping the *shape* and replacing the *content*:

| Keep the shape of | Replace |
|---|---|
| `App` — `RwLock<State>`, `observe`, `mutate` that drops the lock before notifying | `State`'s fields |
| `Document::apply(&Edit) -> Edit` — the inverse falls out of the data | `Document` and `Edit` |
| `history.rs` — two stacks, `mark_saved`, "dirty is a position" | what coalesces, if anything |
| `encoding.rs`'s promise: an untouched save returns the bytes read | the file format |
| `*_file` / `*_bytes` pairs | the format behind them |
| `view.rs`: `View` is only what fits a `Frame`; painting never scrolls | what a row/cell/item is |
| `layout.rs`'s `Metrics` seam, if your document lays out text | the layout itself |
| `command.rs`: verbs as data | the verbs |
| `session.rs`: what a stateless CLI carries between invocations | its fields |

Rules while you do it:

- **Delete what does not apply rather than leaving it unused.** If your document has no text
  layout, `layout.rs` and the `Metrics` implementations go — but then ask what *does* need
  measuring, and whether it is a font question (shell) or a model question (core).
- **Every behaviour a shell would otherwise invent goes here**: selection rules, what Escape means,
  how undo groups, what "modified" means.
- Keep `encoding.rs`'s round-trip test, rewritten for your format. It is the most valuable test in
  the core.

## 5. The CLI, in lock-step

For each method on `App`, a subcommand in `cli/src/main.rs` and a row in `doc/cli-parity.md` —
`cargo test -p <slug>-cli --test parity` tells you which are missing. Rewrite
`examples/sample.sh` to build a sample of *your* document through the CLI, using every subcommand
(`the_sample_script_reaches_every_subcommand` checks), and rewrite `cli/tests/cli.rs` for your
domain. Commit.

## 6. The shells, one at a time

In this order — terminal, GTK, browser, Windows, macOS — and **commit after each**. For each:

1. Make it compile against the new core. Its keymap and menu tests will fail where your verbs and
   motions differ; that is them working.
2. Replace its drawing: `shell.rs` (terminal), `paint.rs` (GTK, macOS), `render` in `lib.rs`
   (browser), `gdi.rs` (Windows). Keep `--render-to`.
3. Keep its conventions, and rewrite its `doc/shells/<name>.md`: what it does, how it is verified,
   and its gaps **named**.
4. Look at it — `scripts/run.sh <shell>`, or the screenshot recipes in `doc/ci.md`. jsdom and
   `--render-to` prove wiring and reproducibility, not appearance.

The macOS shell cannot be linked or run without a Mac. Type-check and lint it for both Apple targets
(`scripts/check.sh` does), keep its portable half tested, and let `artifacts.yml` run it; say in your
summary that it has not been run, if it has not.

## 7. Dropping a shell (only if the person asked)

Remove its directory, its line in `[workspace] members` (and `default-members`), its jobs in
`shells.yml` and `artifacts.yml`, its entry in `cli/tests/packaging.rs`, its `doc/shells/` file, and
its row in the README's table. `cargo test` and the packaging test will find what you missed.

## 8. Identity details the script cannot know

- A display name with spaces: `core/src/lib.rs`'s `APP_NAME`, the `.desktop` file's `Name=`, the
  metainfo `<name>`, `app.rc`'s `ProductName` and `FileDescription`, the Mac menu titles in
  `ui_mac/src/menu.rs`, and `.github/scripts/mac-bundle.sh`'s bundle and volume names (quote them).
- The icon: `ui_gtk/data/<app-id>.svg` is the source; regenerate `ui_win32/data/app.ico` and
  `ui_mac/data/icon.png` from it (the commands are in `app.rc` and `doc/shells/mac.md`).
- File types: the `.desktop` file's `MimeType=`, the Info.plist's `CFBundleDocumentTypes`
  (`ui_mac/src/plist.rs`), the browser's `accept=`, the Win32 open dialog's filter.
- The README: rewrite it for your application. Keep the "Why" if it still is why.

## 9. Finish

- Delete `doc/bootstrap.md`, `scripts/bootstrap.py`, `.claude/skills/bootstrap/`, and the
  `bootstrap` job in `.github/workflows/ci.yml` — they describe the template, not your application.
- Rewrite `AGENTS.md`'s first section for your application; keep the rest, which is the method.
- `scripts/check.sh` green, and every shell looked at. Report what was verified and how, what was
  not (the macOS shell, without a Mac), and every named gap.
