<!--
SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>

SPDX-License-Identifier: MIT
-->

# A tour of the code

Where to start reading, what happens when a key is pressed, and where one capability lives in every
layer. `doc/architecture.md` is the argument for the pattern; this is the map of it. Functions are
named rather than line-numbered, so search for the name.

## Read in this order

| Step | Read | You will know |
|---|---|---|
| 1 | `core/examples/minimal_shell.rs` — and run it | The whole contract between a shell and the core, in one file with no toolkit |
| 2 | `core/src/lib.rs`, then the doc comment at the top of `core/src/app.rs` | What the core owns, and that every `pub fn` on `App` is a capability |
| 3 | `examples/sample.sh` | Every capability, used once, from the command line |
| 4 | `ui_tui/src/main.rs`, `keymap.rs`, `cells.rs`, then `shell.rs` | The smallest real shell: an event loop, platform keys, a prompt, a palette |
| 5 | `doc/architecture.md` | Why each of those is shaped as it is, and the traps |
| 6 | One native shell and its page in `doc/shells/` | What "native" adds: menus, dialogs, a clipboard, a font engine |

```sh
cargo run -p quill-core --example minimal_shell     # step 1
scripts/run.sh tui                                  # step 4, on the sample document
```

Every module opens with a doc comment that is its design note: what it is for, and the decision
that shaped it. `cargo doc --no-deps -p quill-core --open` reads them as one document.

## One keystroke, end to end

Pressing `a` in the terminal shell. Every shell takes these same six steps; the terminal's are the
shortest to read.

```
 key ──▶ keymap ──▶ Action ──▶ App::type_text ──▶ mutate ──▶ Observer::changed
                                                                    │
 screen ◀── paint rows ◀── View ◀── App::view(frame) ◀── event loop sees the flag
```

1. **The platform delivers a key.** `event_loop` in `ui_tui/src/main.rs` blocks in `event::read()`
   and hands the key to `Shell::on_key` in `ui_tui/src/shell.rs`.
2. **The keymap names it.** `keymap::action_for` in `ui_tui/src/keymap.rs` returns
   `Action::Type('a')`. The keymap sees no terminal and no document — key in, `Action` out — which
   is why it is unit-tested like any other function.
3. **The shell forwards it.** `Shell::act` calls `App::type_text`, then `App::follow_caret`. That
   is the shell's whole part in editing: it decides nothing about what typing means.
4. **The core changes and records.** `App::type_text` in `core/src/app.rs` runs under `mutate`:
   `State::replace_selection` builds an `Edit`, `Document::apply` (`core/src/document.rs`) applies
   it, and `History::record` (`core/src/history.rs`) keeps it — and decides that this letter undoes
   together with the rest of its word.
5. **The core announces it.** `mutate` releases the write lock and *then* calls every
   `Observer::changed`. The terminal's observer is `RedrawFlag` in `ui_tui/src/shell.rs`: it sets
   an `AtomicBool` and returns. It does not draw.
6. **The shell paints from the core.** Back in `event_loop`, `redraw.take()` is true, so
   `Shell::draw` asks `App::view` for the rows that fit its `Frame`. The core wraps them
   (`layout::wrap` in `core/src/layout.rs`), asking the shell only how wide text is — `Cells` in
   `ui_tui/src/cells.rs`, the shell's `Metrics`. The `View` that comes back holds the rows, each
   row's selection and the caret, already in terminal cells; `draw` prints them.

Nothing in steps 1–3 or 6 knows what a document is, and nothing in steps 4–5 knows what a terminal
is. That is the pattern.

## The same steps in every shell

| | Key arrives in | Keymap | Forwards in | Observer bridge | `Metrics` | Paints in |
|---|---|---|---|---|---|---|
| minimal | `main` | `parse` | `Shell::act` | `Redraw` | `Columns` | `Shell::paint` |
| terminal | `event_loop`, `main.rs` | `keymap.rs` `action_for` | `shell.rs` `Shell::act` | `RedrawFlag`, `shell.rs` | `Cells`, `cells.rs` | `Shell::draw` |
| GNOME | `wire_keys`, `window.rs` | `keymap.rs` `action_for` | `wire_keys`; verbs in `Ui::run` | `Notifier`, `window.rs` | `PangoMetrics`, `paint.rs` | `paint.rs` `paint` |
| browser | `wire_keyboard`, `lib.rs` | `keymap.rs` `action_for` | `Ui::act`, `Ui::run` | `Notifier`, `lib.rs` | `CanvasMetrics`, `metrics.rs` | `Ui::render` |
| Windows | `WM_KEYDOWN` / `WM_CHAR`, `win.rs` | `keymap.rs` `action_for_key` | `act` and `run`, `win.rs` | `Notifier`, `win.rs` | `GdiMetrics`, `gdi.rs` | `gdi.rs` `paint` |
| macOS | `key_down`, `view.rs` | `keys.rs` `action` | `act` and `run`, `view.rs` | `Notifier`, `view.rs` | `CoreText`, `metrics.rs` | `paint.rs` `paint` |

Paths are relative to the shell's `src/`: `ui_tui/`, `ui_gtk/`, `ui_web/`, `ui_win32/`, `ui_mac/`.
The CLI (`cli/src/main.rs`) has no loop and no observer: each invocation is steps 3 and 4 once,
followed by a report (`cli/src/report.rs`).

## One capability, top to bottom

**Select All** is the smallest complete one. These are all the places it exists — which is also
the list of places a new verb goes, and the check that tells you when one is missing.

| Layer | Where | What tells you it is missing |
|---|---|---|
| The rule | `App::select_all` in `core/src/app.rs`, and its test at the bottom of that file | nothing — this is where you start |
| The CLI | `Cmd::Select` in `cli/src/main.rs` (`quill select FILE --all`) | `cli/tests/parity.rs`: a `pub fn` on `App` with no row |
| The parity row | `select_all` in `doc/cli-parity.md` | the same test |
| The sample | `e select "$doc" --all` in `examples/sample.sh` | `the_sample_script_reaches_every_subcommand` in `cli/tests/cli.rs` |
| The verb | `Command::SelectAll` in `core/src/command.rs`: variant, `ALL`, id, label, summary | the compiler, in every `match` on `Command` below |
| Terminal | `BINDINGS` in `ui_tui/src/keymap.rs` (Ctrl+A); the arm in `Shell::run` | `every_command_has_a_key`; the compiler |
| GNOME | the menu table and `accels` in `ui_gtk/src/menu.rs`; the arm in `Ui::run`, `window.rs` | `every_command_is_in_the_menu_exactly_once` |
| Browser | `ui_web/src/keymap.rs`; the arm in `Ui::run`, `lib.rs` | `every_command_is_reachable` |
| Windows | `ui_win32/src/menu.rs` (`Select &All`, Ctrl+A); the arm in `run`, `win.rs` | `every_command_is_in_the_menu_bar_exactly_once` |
| macOS | `ui_mac/src/menu.rs` (⌘A, the `selectAll:` selector); the arm in `run`, `view.rs` | `every_command_is_in_the_menu_bar_exactly_once` |

Two things to notice. Each shell spells the key its own platform's way, in its own table — the
verb is shared, the key is not. And no shell *implements* selecting everything: each arm is
`app.select_all()`. If an arm grows logic, that logic belongs in the core.

The order to add one in is the order of the table: core and CLI in one change, then the verb, then
let the compiler and the tests name each shell that still lacks it. `doc/architecture.md` §7 is
the same list as a checklist.

## Where do I change…?

| To change | Go to |
|---|---|
| What an edit, a motion or undo *means* | `core/src/` — `app.rs`, `motion.rs`, `history.rs` |
| What a file round-trips as | `core/src/encoding.rs` |
| Where lines wrap, where a click lands | `core/src/layout.rs`, `core/src/view.rs` |
| The list of menu verbs | `core/src/command.rs` |
| A key or a shortcut on one platform | that shell's `keymap.rs` / `keys.rs` / `menu.rs` |
| Colours, fonts, margins on one platform | that shell's `paint.rs` / `gdi.rs` / `theme.rs` / `style.css` |
| A dialog, a title bar, a clipboard | that shell's window module (`shell.rs`, `window.rs`, `lib.rs`, `win.rs`, `view.rs`) |
| What the CLI prints | `cli/src/report.rs` — the only place positions become 1-based |
| The application's name and ID | `core/src/lib.rs` (`APP_NAME`, `APP_ID`); to rename everything, `scripts/bootstrap.py` |
| The whole domain — not a text editor at all | `doc/bootstrap.md` |

If the answer to "where does this go?" is "in two shells", it goes in the core.
