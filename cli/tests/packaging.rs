// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! **Every binary is shipped**, checked by reading the manifests and the release workflow.
//!
//! The bug this prevents happened in grind: a GUI shell was built, tested, linted and documented,
//! and the packaging workflow made packages for every binary but that one — so it was a shell
//! you could run from a checkout and not one anybody could install, and nothing said so. A
//! binary with no package is invisible the way a capability with no CLI verb is, and silently.
//!
//! In `cli/tests/` because the rule is the project's rather than one crate's, and the GTK crate
//! is outside the default build: a guard living there would not run in the job that gates every
//! push. Everything is `include_str!`, read at compile time.

const WORKSPACE: &str = include_str!("../../Cargo.toml");
const WORKFLOW: &str = include_str!("../../.github/workflows/artifacts.yml");

/// A crate that ships a Linux package: its directory, package name, binary, and manifest.
struct Packaged {
    dir: &'static str,
    package: &'static str,
    binary: &'static str,
    manifest: &'static str,
    /// A desktop application also ships a `.desktop` file, metainfo and an icon.
    desktop: bool,
}

const PACKAGED: [Packaged; 3] = [
    Packaged {
        dir: "cli",
        package: "quill-cli",
        binary: "quill",
        manifest: include_str!("../Cargo.toml"),
        desktop: false,
    },
    Packaged {
        dir: "ui_tui",
        package: "quill-tui",
        binary: "quill-tui",
        manifest: include_str!("../../ui_tui/Cargo.toml"),
        desktop: false,
    },
    Packaged {
        dir: "ui_gtk",
        package: "quill-gtk",
        binary: "quill-gtk",
        manifest: include_str!("../../ui_gtk/Cargo.toml"),
        desktop: true,
    },
];

/// Members that ship no Linux package, each with the reason — so a crate here is a decision,
/// not a package somebody forgot.
const ELSEWHERE: [(&str, &str); 4] = [
    ("core", "a library"),
    (
        "ui_web",
        "a wasm bundle served as files; artifacts.yml uploads it",
    ),
    (
        "ui_win32",
        "a Windows executable; artifacts.yml builds it on windows-latest",
    ),
    (
        "ui_mac",
        "a macOS application; artifacts.yml bundles Quill.app and a DMG on macOS",
    ),
];

fn members() -> Vec<&'static str> {
    let line = WORKSPACE
        .lines()
        .find(|line| line.starts_with("members = ["))
        .expect("the workspace lists its members on one line");
    line.split('"').skip(1).step_by(2).collect()
}

#[test]
fn every_member_is_packaged_or_says_why_not() {
    for member in members() {
        let packaged = PACKAGED.iter().any(|p| p.dir == member);
        let elsewhere = ELSEWHERE.iter().any(|(dir, _)| *dir == member);
        assert!(
            packaged ^ elsewhere,
            "workspace member {member} must be in PACKAGED or ELSEWHERE, exactly one"
        );
    }
    for (dir, _) in ELSEWHERE {
        assert!(
            members().contains(&dir),
            "ELSEWHERE names {dir}, not a member any more"
        );
    }
}

#[test]
fn every_packaged_binary_has_both_packages_and_both_workflow_lines() {
    for p in &PACKAGED {
        for block in ["[package.metadata.deb]", "[package.metadata.generate-rpm]"] {
            assert!(
                p.manifest.contains(block),
                "{}/Cargo.toml has no {block}",
                p.dir
            );
        }
        assert!(
            p.manifest
                .contains(&format!("target/release/{}\"", p.binary)),
            "{}'s packages do not ship target/release/{}",
            p.dir,
            p.binary
        );
        // `--no-build`, always: `cargo deb` builds what it packages otherwise, and that is a
        // second release build of a binary this job already built.
        let deb = format!("cargo deb -p {} --no-build", p.package);
        assert!(WORKFLOW.contains(&deb), "artifacts.yml has no `{deb}`");
        let rpm = format!("cargo generate-rpm -p {}", p.dir);
        assert!(WORKFLOW.contains(&rpm), "artifacts.yml has no `{rpm}`");
        let build = WORKFLOW
            .lines()
            .find(|l| l.contains("cargo build --release") && l.contains("-p quill-cli"))
            .expect("one release build of the Linux binaries");
        assert!(
            build.contains(&format!("-p {}", p.package)),
            "{} is not built",
            p.package
        );
    }
}

#[test]
fn a_desktop_application_ships_what_a_desktop_needs() {
    let id = quill_core::APP_ID;
    for p in PACKAGED.iter().filter(|p| p.desktop) {
        for file in [
            format!("data/{id}.desktop"),
            format!("data/{id}.metainfo.xml"),
            format!("data/{id}.svg"),
        ] {
            assert_eq!(
                p.manifest.matches(&file).count(),
                2,
                "{} must ship {file} in both its .deb and its .rpm",
                p.dir
            );
        }
    }
}
