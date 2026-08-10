#!/usr/bin/env bash
# Regenerate the Swift bindings from the built Rust library.
#
# Run after any change to ffi/. The generated files are gitignored, so this has to
# run before the first build on a fresh clone; CI does it too.
#
#   ui_mac/generate-bindings.sh [debug|release]

set -euo pipefail

profile="${1:-release}"
root="$(cd "$(dirname "$0")/.." && pwd)"

case "$(uname -s)" in
    Darwin) library="$root/target/$profile/libeditor_ffi.dylib" ;;
    *)      library="$root/target/$profile/libeditor_ffi.so" ;;
esac

if [ ! -f "$library" ]; then
    echo "missing $library — run: cargo build --$profile -p editor-ffi" >&2
    exit 1
fi

staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT

cargo run -q --manifest-path "$root/Cargo.toml" -p editor-ffi --bin uniffi-bindgen -- \
    generate --library "$library" --language swift --out-dir "$staging"

# EditorCore holds nothing but the generated file, which is gitignored, so on a
# fresh clone the directory does not exist yet.
mkdir -p "$root/ui_mac/Sources/EditorCore" "$root/ui_mac/Sources/EditorFFI/include"

cp "$staging/editor_ffi.swift" "$root/ui_mac/Sources/EditorCore/editor_ffi.swift"
cp "$staging/editor_ffiFFI.h" "$root/ui_mac/Sources/EditorFFI/include/editor_ffiFFI.h"
# The generated module map is intentionally not copied: ours is portable.

echo "bindings written to ui_mac/Sources/"
