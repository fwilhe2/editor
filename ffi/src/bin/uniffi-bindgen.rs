//! The UniFFI bindings generator, built from the same uniffi version as the
//! library it generates for.
//!
//! Used for Swift. C# needs the external `uniffi-bindgen-cs`, whose version has
//! to be kept in step by hand — see `.github/workflows/windows.yml`.

fn main() {
    uniffi::uniffi_bindgen_main()
}
