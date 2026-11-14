// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! **The documentation points at files that exist.**
//!
//! This repository explains itself by naming files: the tour says "read `ui_tui/src/keymap.rs`",
//! a doc comment says "pinned by `cli/tests/parity.rs`". A map like that is worth exactly as much
//! as it is current, and nothing else notices when a file is renamed, a shell is dropped or the
//! template is bootstrapped into another application. So this reads every Markdown file and every
//! Rust source, collects each path written in backticks (and each relative Markdown link), and
//! fails for any that is not there.
//!
//! Only what is unmistakably a path in this repository is checked — a backticked word with a
//! slash whose first segment is a top-level directory, spelled from plain path characters — so
//! `doc/shells/*.md`, `doc/shells/<name>.md` and `target/release/quill` are left alone.
//!
//! In `cli/tests/` for the reason `packaging.rs` is: the rule is the project's, and this crate's
//! tests run in the job that gates every push.

use std::fs;
use std::path::{Path, PathBuf};

/// The top-level directories a documented path may start with. Build output (`target/`,
/// `ui_web/dist/`) is deliberately absent: it is not there on a fresh checkout.
const ROOTS: [&str; 12] = [
    ".claude", ".github", "cli", "core", "doc", "examples", "scripts", "ui_gtk", "ui_mac",
    "ui_tui", "ui_web", "ui_win32",
];

/// Paths the documentation names that are generated, and so absent from a fresh checkout.
const GENERATED: [&str; 1] = ["ui_web/dist"];

/// Fewer than this and the scan is broken, not the documentation clean — a check that matched
/// nothing would pass vacuously, which is the failure worth guarding against.
const AT_LEAST: usize = 100;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the CLI crate sits one level below the workspace root")
        .to_path_buf()
}

/// Every `.md` and `.rs` file in the repository, skipping build output and the decision records.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("a readable directory") {
        let path = entry.expect("a readable entry").path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            // `doc/decisions/` is skipped on purpose: a decision record is history, kept as
            // written, and names the files that existed when it was made.
            if !matches!(
                name.as_str(),
                "target" | ".git" | "node_modules" | "dist" | "decisions"
            ) {
                sources(&path, out);
            }
        } else if name.ends_with(".md") || name.ends_with(".rs") {
            out.push(path);
        }
    }
}

fn is_repo_path(word: &str) -> bool {
    let Some((first, _)) = word.split_once('/') else {
        return false;
    };
    ROOTS.contains(&first)
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-/".contains(c))
        && !GENERATED.iter().any(|g| word.starts_with(g))
}

/// The backticked paths in `text`, and — in Markdown — the targets of its relative links, each
/// resolved against the repository root.
fn references(file: &Path, text: &str, root: &Path) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    // Odd-numbered pieces of a split on backticks are the spans inside them.
    for word in text.split('`').skip(1).step_by(2) {
        if is_repo_path(word) {
            found.push((word.to_string(), root.join(word)));
        }
    }
    if file.extension().is_some_and(|e| e == "md") {
        for rest in text.split("](").skip(1) {
            let target = rest.split([')', '#', ' ']).next().unwrap_or_default();
            if !target.is_empty() && !target.contains("://") && !target.starts_with("mailto:") {
                found.push((target.to_string(), file.parent().unwrap().join(target)));
            }
        }
    }
    found
}

#[test]
fn every_path_the_documentation_names_exists() {
    let root = root();
    let mut files = Vec::new();
    sources(&root, &mut files);

    let mut checked = 0;
    let mut missing = Vec::new();
    for file in &files {
        let text = fs::read_to_string(file).expect("a UTF-8 source file");
        for (written, path) in references(file, &text, &root) {
            checked += 1;
            if !path.exists() {
                let from = file.strip_prefix(&root).unwrap().display();
                missing.push(format!("{from} names `{written}`"));
            }
        }
    }

    assert!(
        checked >= AT_LEAST,
        "only {checked} paths found in {} files — the scan is broken",
        files.len()
    );
    assert!(
        missing.is_empty(),
        "the documentation names files that do not exist — fix the reference, or the rename \
         that orphaned it:\n  {}",
        missing.join("\n  ")
    );
}

#[test]
fn only_real_paths_are_checked() {
    assert!(is_repo_path("core/src/app.rs"));
    assert!(is_repo_path("doc/decisions/"));
    assert!(!is_repo_path("doc/shells/*.md"));
    assert!(!is_repo_path("doc/shells/<name>.md"));
    assert!(!is_repo_path("target/release/quill"));
    assert!(!is_repo_path("ui_web/dist/index.html"));
    assert!(!is_repo_path("app.rs"));
}
