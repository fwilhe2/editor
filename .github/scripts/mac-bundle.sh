#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
#
# SPDX-License-Identifier: MIT
#
# mac-bundle.sh <quill-mac arm64> <quill-mac x86_64> <out>
#
# Quill.app from the two slices, signed ad hoc, in a DMG — and every claim about it checked on
# what was built rather than on what was meant:
#
#   * the two slices are joined with `lipo`, and both are there;
#   * the Info.plist is `quill-mac --info-plist`'s, and `plutil -lint` passes it;
#   * the icon is made here, from ui_mac/data/icon.png, with the platform's own `sips` and
#     `iconutil` — the tools exist only on a Mac, which is where this runs;
#   * `codesign --verify --strict` passes the ad-hoc signature;
#   * `otool -L` lists only /System/Library and /usr/lib: nothing the app needs is missing from
#     a clean Mac (the Mac's version of the Windows import-table check);
#   * `vtool` reports the deployment target for both slices;
#   * `spctl` *rejects* the ad-hoc build, as expected — the day a Developer ID is added this
#     flips, and the check should then be made to expect it.

set -euo pipefail

arm="$1"
intel="$2"
out="$3"
minimum="15.0"
mkdir -p "$out"

fail() {
    echo "mac-bundle: $*" >&2
    exit 1
}

app="$out/Quill.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

lipo -create "$arm" "$intel" -output "$app/Contents/MacOS/quill-mac"
archs="$(lipo -archs "$app/Contents/MacOS/quill-mac")"
case "$archs" in
    *arm64*x86_64* | *x86_64*arm64*) echo "universal: $archs" ;;
    *) fail "the binary is not universal: $archs" ;;
esac

"$arm" --info-plist > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" > /dev/null || fail "plutil rejects the Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"

iconset="$out/Quill.iconset"
rm -rf "$iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" ui_mac/data/icon.png --out "$iconset/icon_${size}x${size}.png" > /dev/null
    double=$((size * 2))
    sips -z "$double" "$double" ui_mac/data/icon.png --out "$iconset/icon_${size}x${size}@2x.png" > /dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/Quill.icns"
echo "icon: $(du -h "$app/Contents/Resources/Quill.icns" | cut -f1)"

codesign --force --sign - --timestamp=none "$app"
codesign --verify --strict --verbose=2 "$app" || fail "codesign --verify --strict"
echo "signed ad hoc"

foreign="$(otool -L "$app/Contents/MacOS/quill-mac" | grep '^[[:space:]]' | awk '{ print $1 }' \
    | grep -v -e '^/System/Library/' -e '^/usr/lib/' || true)"
[ -z "$foreign" ] || fail "the binary links outside the system: $foreign"
echo "otool -L: /System/Library and /usr/lib only"

for arch in arm64 x86_64; do
    minos="$(vtool -arch "$arch" -show-build "$app/Contents/MacOS/quill-mac" | awk '/minos/ { print $2; exit }')"
    [ "$minos" = "$minimum" ] || fail "$arch's minimum is $minos, not $minimum"
done
echo "vtool: minimum $minimum for both slices"

if spctl --assess --type execute "$app" 2> /dev/null; then
    fail "spctl accepts an ad-hoc build — a Developer ID was added: make this check expect it"
fi
echo "spctl rejects the ad-hoc build, as expected"

stage="$out/dmg"
rm -rf "$stage"
mkdir -p "$stage"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
hdiutil create -quiet -volname Quill -srcfolder "$stage" -ov -format UDZO "$out/Quill.dmg"
echo "Quill.dmg: $(du -h "$out/Quill.dmg" | cut -f1)"
