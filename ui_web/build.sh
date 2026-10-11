#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
#
# SPDX-License-Identifier: MIT
#
# Build the browser shell into ui_web/dist.
#
#   ui_web/build.sh [debug|release]
#
# Two steps, because rustc produces only half of what a page needs: a `.wasm` module, and then
# the `wasm-bindgen` CLI writes the JavaScript glue that loads it. The CLI's version must equal
# the `wasm-bindgen` crate's or the glue will not load the module — so this script reads the
# version cargo resolved out of Cargo.lock and refuses to run on a mismatch, rather than
# pinning it in prose that goes stale.
#
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version <the version this script asks for>

set -euo pipefail

profile="${1:-release}"
root="$(cd "$(dirname "$0")/.." && pwd)"
dist="$root/ui_web/dist"
target_dir="${CARGO_TARGET_DIR:-$root/target}"

case "$profile" in
    debug) flags=() ;;
    release) flags=(--release) ;;
    *) echo "usage: $(basename "$0") [debug|release]" >&2; exit 2 ;;
esac

# The exact-name match keeps wasm-bindgen-futures and friends from answering.
wanted="$(grep -A1 '^name = "wasm-bindgen"$' "$root/Cargo.lock" \
    | sed -n 's/^version = "\(.*\)"/\1/p' | head -n1)"
if ! command -v wasm-bindgen >/dev/null 2>&1; then
    echo "wasm-bindgen CLI not found — cargo install wasm-bindgen-cli --version $wanted" >&2
    exit 1
fi
have="$(wasm-bindgen --version | awk '{print $2}')"
if [ "$have" != "$wanted" ]; then
    echo "wasm-bindgen CLI is $have but the crate is $wanted; the glue would not load." >&2
    echo "cargo install wasm-bindgen-cli --version $wanted" >&2
    exit 1
fi

cargo build -p quill-web --target wasm32-unknown-unknown "${flags[@]}"

rm -rf "$dist"
mkdir -p "$dist"
wasm-bindgen --target web --no-typescript --out-dir "$dist" --out-name quill_web \
    "$target_dir/wasm32-unknown-unknown/$profile/quill_web.wasm"
cp "$root/ui_web/index.html" "$root/ui_web/style.css" "$dist/"

echo "ui_web/dist is ready. Serve it — browsers refuse ES modules and wasm over file://:"
echo "  python3 -m http.server --directory ui_web/dist 8000"
