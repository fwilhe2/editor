// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! The icon, version block and manifest (`data/app.rc`), compiled and linked in by
//! `embed_resource` — and never attempted off Windows.
//!
//! `CARGO_CFG_TARGET_OS` rather than `cfg!(windows)`: a build script runs on the *host*, so
//! `cfg!` would answer for the machine doing the build, and this crate is routinely checked for
//! `x86_64-pc-windows-msvc` from Linux. Where no resource compiler exists for the target,
//! `embed_resource` degrades to a no-op rather than an error, which is what keeps `cargo check
//! --target x86_64-pc-windows-msvc` working with no Windows at all; with `llvm-windres`
//! installed, `cargo xwin build` embeds them for real. The shipped claim is checked on
//! `windows-latest` only, where `artifacts.yml` reads the version block back off the `.exe`.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        for input in ["data/app.rc", "data/app.manifest", "data/app.ico"] {
            println!("cargo:rerun-if-changed={input}");
        }
        let result = embed_resource::compile("data/app.rc", embed_resource::NONE);
        // Printed whatever happened: `manifest_optional` swallows "not attempted" and "done"
        // alike, and a silently missing resource is otherwise a mystery on the runner.
        println!("cargo:warning=app.rc: {result}");
        if let Err(failed) = result.manifest_optional() {
            println!("cargo:warning=app.rc failed to compile: {failed}");
        }
    }
}
