<!--
SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>

SPDX-License-Identifier: MIT
-->

# CLI parity

**Every public method of `quill_core::App` and the command that reaches it.** Rule 5 of
`doc/architecture.md` — *whatever any window can do, the CLI can do* — made mechanical:
`cli/tests/parity.rs` reads `core/src/app.rs`, lists every `pub fn` in `impl App`, and fails
the build when a method has no row here, when a row names a method that no longer exists, or
when a row says `not exposed:` without a real reason.

So adding a capability is three steps in one change: the method in the core, the command in
`cli/src/main.rs`, and the row below. A row is `` - `method` — how ``.

## The document

- `new` — every command constructs one, then opens the file into it.
- `new_document` — `quill new FILE`.
- `open_file` — every command that takes a `FILE`.
- `open_bytes` — `quill import FILE SOURCE` (`-` reads stdin): bytes in with no path, as a browser's file picker hands them over.
- `save_file` — every command that changes the document writes it back (`"written": true`).
- `save_file_as` — `quill save-as FILE TO`, and `quill new` / `quill import`.
- `save_bytes` — `quill export FILE`: exactly the bytes Save would write.

## Editing

- `type_text` — `quill type FILE --text T`, one character at a time, as a keyboard sends them.
- `insert_text` — `quill insert FILE --text T [--at L:C]`, as one change — the paste path.
- `delete` — `quill delete FILE [backward|forward|word-backward|word-forward] [--count N]`.
- `undo` — `quill --session S undo FILE`.
- `redo` — `quill --session S redo FILE`.
- `can_undo` — `quill info FILE` (`can_undo`).
- `can_redo` — `quill info FILE` (`can_redo`).

## The caret and the selection

- `move_caret` — `quill move FILE MOTION [--extend] [--count N] [--width W] [--rows R]`.
- `set_caret` — `quill goto FILE L:C [--extend]`, and `quill insert --at`.
- `place_caret` — `quill click FILE X Y [--extend] [--width W]`: a click, in columns and rows.
- `select` — `quill select FILE FROM TO`.
- `select_all` — `quill select FILE --all`.
- `clear_selection` — `quill select FILE --none`.
- `caret` — every state report (`caret`).
- `selection` — every state report (`selection`).
- `selected_text` — `quill copy FILE`.
- `cut` — `quill cut FILE`.
- `find` — `quill find FILE QUERY [--backward]`.
- `find_again` — `quill --session S find-next FILE [--backward]`.
- `search` — `quill info FILE` (`search`), with a session.

## What is on screen

- `view` — `quill view FILE --width W [--rows R]`: the rows a window that wide would draw.
- `follow_caret` — `quill move` (after moving, as a window does) and `quill view --follow`.
- `scroll_by` — `quill scroll FILE --by N [--width W]`.
- `scroll` — every state report (`scroll`).
- `scroll_to` — `quill scroll FILE --to LINE`, and `quill view --from LINE --width W`.
- `lines` — `quill view FILE [--from A] [--to B]` without `--width`.

## About the document

- `line_count` — every state report (`lines`).
- `char_count` — every state report (`chars`).
- `is_dirty` — every state report (`dirty`).
- `path` — every state report (`path`).
- `title` — every state report (`title`).
- `encoding` — every state report (`line_ending`, `bom`).

## Sessions

- `session` — `--session S` writes one after every command.
- `restore_session` — `--session S` reads one before every command.
- `observe` — not exposed: an observer is how a long-lived window learns it must repaint, and a CLI process exits after one command with nothing on screen to repaint.

## Beyond `App`

Free functions and types a shell uses that are not capabilities of the document, listed so the
next reader does not go looking for their command:

- `Command::ALL` — `quill commands` prints the table every window's menus are built from.
- `command::score` — the palette's ranking; the browser shell's Ctrl+K is its only caller.
