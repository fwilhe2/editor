#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
#
# SPDX-License-Identifier: MIT
#
# Every gate CI has that this machine can run, in CI's order, before you push.
#
#   scripts/check.sh
#
# A gate whose tool is missing here is *skipped and said so* rather than passed: the summary at
# the end lists what ran and what did not, so "it passed locally" means what it says. The
# release builds, the Windows and macOS links and the packaging are artifacts.yml's, on their own
# operating systems, and are not attempted here.

set -uo pipefail
cd "$(dirname "$0")/.."

crates=(-p quill-core -p quill-cli -p quill-tui -p quill-web -p quill-win32 -p quill-mac)
ran=()
skipped=()
failed=()

gate() {
    local name="$1"
    shift
    echo "== $name"
    if "$@"; then ran+=("$name"); else failed+=("$name"); fi
}

skip() {
    echo "== $1 — skipped: $2"
    skipped+=("$1 ($2)")
}

has_target() {
    rustup target list --installed 2> /dev/null | grep -qx "$1"
}

gate fmt cargo fmt --all --check
gate clippy cargo clippy --locked "${crates[@]}" --all-targets -- -D warnings
gate test cargo test --locked "${crates[@]}"
gate docs env RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked "${crates[@]}"

if pkg-config --exists gtk4 libadwaita-1 2> /dev/null; then
    gate gtk cargo test --locked -p quill-gtk
    gate gtk-clippy cargo clippy --locked -p quill-gtk --all-targets -- -D warnings
else
    skip gtk "no libgtk-4-dev / libadwaita-1-dev"
fi

if has_target x86_64-pc-windows-msvc; then
    gate win32-from-linux cargo clippy --locked -p quill-win32 --target x86_64-pc-windows-msvc --all-targets -- -D warnings
else
    skip win32-from-linux "rustup target add x86_64-pc-windows-msvc"
fi

if has_target aarch64-apple-darwin && has_target x86_64-apple-darwin; then
    gate mac-from-linux sh -c 'for t in aarch64-apple-darwin x86_64-apple-darwin; do cargo clippy --locked -p quill-mac --all-targets --target "$t" -- -D warnings || exit 1; done'
else
    skip mac-from-linux "rustup target add aarch64-apple-darwin x86_64-apple-darwin"
fi

if has_target wasm32-unknown-unknown && command -v wasm-bindgen > /dev/null && command -v node > /dev/null; then
    gate web sh -c 'ui_web/build.sh debug > /dev/null && ui_web/smoke.sh debug'
else
    skip web "wasm32 target, wasm-bindgen-cli or node missing"
fi

if command -v cargo-deny > /dev/null; then
    gate licences cargo deny --locked check licenses bans sources
else
    skip licences "cargo install cargo-deny"
fi

if command -v reuse > /dev/null; then
    gate reuse reuse lint
elif command -v podman > /dev/null; then
    gate reuse podman run --rm -v "$PWD:/data:Z" docker.io/fsfe/reuse:latest lint
else
    skip reuse "no reuse and no podman"
fi

echo
echo "ran:     ${ran[*]:-none}"
echo "skipped: ${skipped[*]:-none}"
if [ ${#failed[@]} -gt 0 ]; then
    echo "FAILED:  ${failed[*]}"
    exit 1
fi
echo "all gates that could run here passed"
