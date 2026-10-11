// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! The parity ratchet: every capability of the core is reachable from the CLI.
//!
//! Rust cannot reflect over its own API, so this reads the source instead and checks it against
//! `doc/cli-parity.md`. Adding a method to `App` without saying how a user reaches it fails the
//! build; so does leaving a row behind for a method that no longer exists.
//!
//! Both files are `include_str!`, read at *compile* time, so the test cannot pass by looking in
//! the wrong place at run time.

const CORE: &str = include_str!("../../core/src/app.rs");
const PARITY: &str = include_str!("../../doc/cli-parity.md");

/// The smallest API that is plausibly complete. A scan that matched nothing would pass
/// vacuously and quietly retire the ratchet — the failure actually worth guarding against.
const AT_LEAST: usize = 30;

/// Every `pub fn` in `impl App`.
fn public_methods() -> Vec<&'static str> {
    let body = CORE
        .split_once("impl App {")
        .expect("core/src/app.rs still defines `impl App`")
        .1;
    // rustfmt puts a top-level impl's closing brace in column 0, and nothing inside the block
    // is indented that little.
    let body = body.split_once("\n}").expect("the impl block ends").0;
    body.lines()
        .filter_map(|line| line.strip_prefix("    pub fn "))
        .filter_map(|rest| rest.split(['(', '<']).next())
        .collect()
}

/// Every method named in the parity document, with the text saying how it is reached.
fn documented(section: &str) -> Vec<(&str, &str)> {
    section
        .lines()
        .filter_map(|line| line.trim().strip_prefix("- `"))
        .filter_map(|rest| rest.split_once('`'))
        .map(|(name, how)| (name, how.trim_start_matches([' ', '—']).trim()))
        .collect()
}

fn app_rows() -> &'static str {
    PARITY
        .split_once("## Beyond `App`")
        .expect("doc/cli-parity.md keeps its trailing section")
        .0
}

#[test]
fn every_core_capability_is_reachable_from_the_cli() {
    let methods = public_methods();
    assert!(
        methods.len() >= AT_LEAST,
        "only found {} methods on App — the scan is broken, not the API",
        methods.len()
    );
    let rows = documented(app_rows());
    for method in &methods {
        let (_, how) = rows
            .iter()
            .find(|(name, _)| name == method)
            .unwrap_or_else(|| {
                panic!(
                    "App::{method} is not in doc/cli-parity.md — give it a `quill` command, \
                     or say there why it has none"
                )
            });
        assert!(!how.is_empty(), "App::{method} has no explanation");
        if let Some(reason) = how.strip_prefix("not exposed:") {
            assert!(
                reason.trim().len() > 40,
                "App::{method} is exempted without a real reason: {how:?}"
            );
        }
    }
}

#[test]
fn the_parity_document_names_no_method_that_is_gone() {
    let methods = public_methods();
    for (name, _) in documented(app_rows()) {
        assert!(
            methods.contains(&name),
            "doc/cli-parity.md lists App::{name}, which no longer exists — delete the row"
        );
    }
}
