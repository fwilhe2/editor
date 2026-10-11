#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
#
# SPDX-License-Identifier: MIT
#
# Build a shell and run it on the sample document — the "see it working" loop.
#
#   scripts/run.sh tui|gtk|web|win32 [document]
#
# With no document it builds one with examples/sample.sh, which uses every capability the
# build has, so whatever just changed is on screen somewhere. The document goes to $QUILL_DEMO
# (default /tmp/quill-demo) and is rebuilt on every run: a stale demo is worse than none.
#
# `win32` links the real Windows binary with `cargo xwin` and runs it under Wine — an inspection
# aid, never a build path: what ships comes off windows-latest in artifacts.yml. It earns its
# place in grind, where several bugs were one glance at a Wine screenshot and invisible in review.
# There is no `mac`: AppKit has no Wine. The macOS frames come from artifacts.yml's runners.

set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
shell="${1:-tui}"
demo="${QUILL_DEMO:-/tmp/quill-demo}"
port="${PORT:-8000}"
target_dir="${CARGO_TARGET_DIR:-$root/target}"

document="${2:-}"
if [ -z "$document" ]; then
    cargo build -q -p quill-cli
    QUILL_BIN="$target_dir/debug/quill" "$root/examples/sample.sh" "$demo" > /dev/null 2>&1
    document="$demo/sample.txt"
fi

case "$shell" in
    tui) exec cargo run -q -p quill-tui -- "$document" ;;
    gtk) exec cargo run -q -p quill-gtk -- "$document" ;;
    web)
        "$root/ui_web/build.sh" debug
        echo "  http://localhost:$port/ — open $document with the Open button"
        exec python3 -m http.server --directory "$root/ui_web/dist" "$port"
        ;;
    win32)
        for tool in cargo-xwin wine; do
            command -v "$tool" > /dev/null || {
                echo "$(basename "$0"): $tool is not on PATH (cargo install cargo-xwin; dnf/apt install wine)" >&2
                exit 1
            }
        done
        cargo xwin build -p quill-win32 --target x86_64-pc-windows-msvc
        export WINEDLLOVERRIDES="${WINEDLLOVERRIDES:-mscoree,mshtml=}" WINEDEBUG="${WINEDEBUG:--all}"
        exec wine "$target_dir/x86_64-pc-windows-msvc/debug/quill-win32.exe" "$document"
        ;;
    *)
        echo "usage: $(basename "$0") tui|gtk|web|win32 [document]" >&2
        exit 2
        ;;
esac
