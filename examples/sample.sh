#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
#
# SPDX-License-Identifier: MIT
#
# examples/sample.sh [DIR] — build a sample document out of every capability, through the CLI.
#
# It is three things at once: a demonstration that everything a window can do is scriptable
# (doc/architecture.md, rule 5), the document `scripts/run.sh` opens in every shell, and a test
# — `cli/tests/cli.rs` runs it, so a capability without a line here is a capability nobody
# exercises end to end. **Add a line when you add a capability.**
#
# $QUILL_BIN is the binary to use (default: `quill` on PATH).

set -euo pipefail

dir="${1:-/tmp/quill-demo}"
bin="${QUILL_BIN:-quill}"
mkdir -p "$dir"
doc="$dir/sample.txt"
session="$dir/.sample.session"
rm -f "$doc" "$session"

# The trace goes to stderr, so a command's own output can be captured.
e() {
    echo "+ quill $*" >&2
    "$bin" --session "$session" "$@"
}

e new "$doc" --force
# Typed a character at a time — undo takes back a word, not a letter.
e type "$doc" --text "A shared core, native shells."
e undo "$doc"
e type "$doc" --text " shells, and a CLI that can do everything they can."
e redo "$doc" || true
e insert "$doc" --text $'\n\nThis line is long enough to wrap in a narrow window, which is where Up, Down, Home and End stop meaning the same thing as the file\'s own lines — so the core decides what they mean, and every shell agrees.'
# The caret, moved the way a window moves it: by rows on screen at a width.
e move "$doc" document-start
e move "$doc" down --count 3 --width 40
e move "$doc" row-end --width 40
e move "$doc" word-left --extend --count 2
# The clipboard belongs to the shell — here, a shell variable.
clipboard="$(e cut "$doc")"
[ "$clipboard" = "in a" ] || { echo "sample.sh: cut gave '$clipboard'" >&2; exit 1; }
e move "$doc" document-end
e insert "$doc" --text $'\n\nCut and pasted back: '"$clipboard"
e goto "$doc" 1:1
e select "$doc" 1:3 1:9
copied="$(e copy "$doc")"
e move "$doc" document-end
e insert "$doc" --text ", and copied: $copied"
# Find, and find again with the same query.
e find "$doc" "core"
e find-next "$doc"
e select "$doc" --none
e delete "$doc" word-backward
e type "$doc" --text "core"
# A click, in columns and rows of a 40-column frame.
e click "$doc" 4 1 --width 40
e scroll "$doc" --by 1 --width 40
e scroll "$doc" --to 1
e view "$doc" --width 40 --rows 12
e view "$doc" --from 1 --to 2
e select "$doc" --all
e copy "$doc" > /dev/null
e select "$doc" --none
# Bytes in and out: a CRLF file imported keeps its line endings when saved.
# (No session: a session belongs to one file.)
printf 'windows\r\nline endings\r\n' > "$dir/crlf-source.txt"
"$bin" new "$dir/crlf.txt" --force
"$bin" import "$dir/crlf.txt" "$dir/crlf-source.txt"
"$bin" insert "$dir/crlf.txt" --text "still " --at 2:1
"$bin" export "$dir/crlf.txt" | od -c | head -n 2
e save-as "$doc" "$dir/sample-copy.txt"
e --format json info "$doc"
e commands > /dev/null

echo "wrote $doc"
