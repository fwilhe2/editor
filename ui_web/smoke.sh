#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
#
# SPDX-License-Identifier: MIT
#
# The browser boundary's smoke test (ui_web/smoke.js): the real `.wasm`, glued for node rather
# than the web, driven against the real index.html in jsdom. No browser, so it runs anywhere —
# and before the bundle is uploaded in CI, so a red job says "wasm, glue or wiring" rather than
# "CSS".
#
#   ui_web/smoke.sh [debug|release]

set -euo pipefail

profile="${1:-release}"
root="$(cd "$(dirname "$0")/.." && pwd)"
target_dir="${CARGO_TARGET_DIR:-$root/target}"
work="$root/ui_web/.smoke"
module="$target_dir/wasm32-unknown-unknown/$profile/quill_web.wasm"

if [ ! -f "$module" ]; then
    echo "missing $module — run ui_web/build.sh $profile first" >&2
    exit 1
fi

mkdir -p "$work"
wasm-bindgen --target nodejs --no-typescript --out-dir "$work" --out-name quill_web "$module"

# Pinned, and installed beside the shell rather than at the root: test scaffolding, not
# something the page ships.
if [ ! -d "$root/ui_web/node_modules/jsdom" ]; then
    npm install --silent --no-save --no-fund --no-audit --prefix "$root/ui_web" jsdom@24
fi

node "$root/ui_web/smoke.js"
