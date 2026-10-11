// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! The bundle's `Info.plist`, generated from the core's identity rather than kept as a second
//! copy of it: `quill-mac --info-plist` prints it, and `.github/scripts/mac-bundle.sh` puts it
//! in `Quill.app` and has `plutil -lint` read it back.
//!
//! The application *offers* to open plain text and never claims to own it: `LSHandlerRank` is
//! `Alternate`, so a double-click keeps opening whatever the person chose, and Quill appears in
//! Open With. Taking a file type's default from the user is not a template's business.

/// The oldest macOS this shell runs on; `.cargo/config.toml` builds for it, and the bundle
/// script checks both slices say so.
pub const MINIMUM: &str = "15.0";

/// The `Info.plist`, as XML.
pub fn info_plist() -> String {
    let name = quill_core::APP_NAME;
    let id = quill_core::APP_ID;
    let version = quill_core::VERSION;
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleDisplayName</key>
	<string>{name}</string>
	<key>CFBundleExecutable</key>
	<string>quill-mac</string>
	<key>CFBundleIconFile</key>
	<string>{name}</string>
	<key>CFBundleIdentifier</key>
	<string>{id}</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>{name}</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>{version}</string>
	<key>CFBundleVersion</key>
	<string>{version}</string>
	<key>LSMinimumSystemVersion</key>
	<string>{MINIMUM}</string>
	<key>LSApplicationCategoryType</key>
	<string>public.app-category.productivity</string>
	<key>NSHighResolutionCapable</key>
	<true/>
	<key>NSSupportsAutomaticTermination</key>
	<false/>
	<key>CFBundleDocumentTypes</key>
	<array>
		<dict>
			<key>CFBundleTypeName</key>
			<string>Plain Text</string>
			<key>CFBundleTypeRole</key>
			<string>Quill</string>
			<key>LSHandlerRank</key>
			<string>Alternate</string>
			<key>LSItemContentTypes</key>
			<array>
				<string>public.plain-text</string>
			</array>
		</dict>
	</array>
</dict>
</plist>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundle_is_named_from_the_core() {
        let plist = info_plist();
        assert!(plist.contains(&format!("<string>{}</string>", quill_core::APP_ID)));
        assert!(
            plist.contains("<string>quill-mac</string>"),
            "the executable's name"
        );
        assert!(
            plist.contains("<string>Alternate</string>"),
            "offered, never taken"
        );
    }

    #[test]
    fn every_key_has_a_value() {
        let plist = info_plist();
        let keys = plist.matches("<key>").count();
        let values = plist.matches("<string>").count()
            + plist.matches("<true/>").count()
            + plist.matches("<false/>").count()
            + plist.matches("<array>").count();
        // The document type's array of content types holds one string that is not a value of
        // a key of its own.
        assert_eq!(keys + 1, values);
    }
}
