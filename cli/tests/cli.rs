// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! The CLI end to end: the real binary, real files, real output.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_quill");

/// A scratch directory per test, removed when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("quill-cli-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

/// Run and insist on success; return stdout.
fn ok(args: &[&str]) -> String {
    let out = run(args);
    assert!(
        out.status.success(),
        "quill {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn json(args: &[&str]) -> serde_json::Value {
    let mut all = vec!["--format", "json"];
    all.extend_from_slice(args);
    serde_json::from_str(&ok(&all)).unwrap()
}

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn typing_writes_the_file_and_reports_a_one_based_caret() {
    let dir = Scratch::new("type");
    let file = dir.path("a.txt");
    ok(&["new", s(&file)]);
    let state = json(&["type", s(&file), "--text", "hello"]);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello");
    assert_eq!(state["caret"]["line"], 1);
    assert_eq!(state["caret"]["column"], 6);
    assert_eq!(state["written"], true);
}

#[test]
fn a_session_carries_history_and_typing_undoes_a_word_at_a_time() {
    let dir = Scratch::new("session");
    let (file, session) = (dir.path("a.txt"), dir.path("s.json"));
    ok(&["new", s(&file)]);
    ok(&[
        "--session",
        s(&session),
        "type",
        s(&file),
        "--text",
        "one two",
    ]);
    ok(&["--session", s(&session), "undo", s(&file)]);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one");
    ok(&["--session", s(&session), "redo", s(&file)]);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one two");
}

#[test]
fn undo_without_a_session_fails_loudly() {
    let dir = Scratch::new("no-session");
    let file = dir.path("a.txt");
    ok(&["new", s(&file)]);
    let out = run(&["undo", s(&file)]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--session"));
}

#[test]
fn a_session_for_a_file_that_changed_is_ignored() {
    let dir = Scratch::new("stale");
    let (file, session) = (dir.path("a.txt"), dir.path("s.json"));
    ok(&["new", s(&file)]);
    ok(&["--session", s(&session), "type", s(&file), "--text", "abc"]);
    std::fs::write(&file, "something else entirely").unwrap();
    let out = run(&["--session", s(&session), "undo", s(&file)]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("changed since"));
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "something else entirely",
        "an undo recorded against other text must not be replayed"
    );
}

#[test]
fn down_moves_by_rows_at_the_width_given() {
    let dir = Scratch::new("down");
    let file = dir.path("a.txt");
    std::fs::write(&file, "hello world\nnext").unwrap();
    let session = dir.path("s.json");
    let s1 = json(&[
        "--session",
        s(&session),
        "move",
        s(&file),
        "down",
        "--width",
        "6",
    ]);
    assert_eq!(s1["caret"]["line"], 1, "the wrapped row of the same line");
    assert_eq!(s1["caret"]["column"], 7);
    let s2 = json(&["--session", s(&session), "move", s(&file), "down"]);
    assert_eq!(
        s2["caret"]["line"], 2,
        "without --width the line is one row"
    );
}

#[test]
fn view_with_a_width_prints_the_rows_a_window_would_draw() {
    let dir = Scratch::new("view");
    let file = dir.path("a.txt");
    std::fs::write(&file, "hello world\nnext").unwrap();
    assert_eq!(
        ok(&["view", s(&file), "--width", "6"]),
        "1  hello \n   world\n2  next\n"
    );
    assert_eq!(ok(&["view", s(&file), "--from", "2"]), "2  next\n");
}

#[test]
fn crlf_and_a_bom_survive_an_edit() {
    let dir = Scratch::new("crlf");
    let file = dir.path("a.txt");
    std::fs::write(&file, b"\xEF\xBB\xBFone\r\ntwo\r\n").unwrap();
    ok(&["insert", s(&file), "--text", "!", "--at", "1:4"]);
    assert_eq!(
        std::fs::read(&file).unwrap(),
        b"\xEF\xBB\xBFone!\r\ntwo\r\n"
    );
    assert_eq!(json(&["info", s(&file)])["line_ending"], "CRLF");
}

#[test]
fn a_file_that_is_not_utf8_is_refused_and_left_alone() {
    let dir = Scratch::new("latin1");
    let file = dir.path("a.txt");
    std::fs::write(&file, b"caf\xE9").unwrap();
    let out = run(&["insert", s(&file), "--text", "x"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not UTF-8"));
    assert_eq!(std::fs::read(&file).unwrap(), b"caf\xE9");
}

#[test]
fn cut_prints_the_selection_and_removes_it() {
    let dir = Scratch::new("cut");
    let file = dir.path("a.txt");
    std::fs::write(&file, "hello world").unwrap();
    let session = dir.path("s.json");
    ok(&["--session", s(&session), "select", s(&file), "1:6", "1:12"]);
    assert_eq!(ok(&["--session", s(&session), "copy", s(&file)]), " world");
    assert_eq!(ok(&["--session", s(&session), "cut", s(&file)]), " world");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello");
}

#[test]
fn find_selects_the_match_and_find_next_repeats_it() {
    let dir = Scratch::new("find");
    let file = dir.path("a.txt");
    std::fs::write(&file, "ab ab ab").unwrap();
    let session = dir.path("s.json");
    let first = json(&["--session", s(&session), "find", s(&file), "ab"]);
    assert_eq!(first["selection"][0]["column"], 1);
    let next = json(&["--session", s(&session), "find-next", s(&file)]);
    assert_eq!(next["selection"][0]["column"], 4);
    assert_eq!(next["search"], "ab");
}

#[test]
fn export_and_import_are_the_bytes_save_would_write() {
    let dir = Scratch::new("bytes");
    let (file, source) = (dir.path("a.txt"), dir.path("b.txt"));
    std::fs::write(&source, "from\r\nelsewhere\r\n").unwrap();
    ok(&["new", s(&file)]);
    ok(&["import", s(&file), s(&source)]);
    assert_eq!(ok(&["export", s(&file)]), "from\r\nelsewhere\r\n");
}

#[test]
fn commands_lists_every_menu_verb() {
    let listed = ok(&["commands"]);
    for command in quill_core::Command::ALL {
        assert!(listed.contains(command.id()), "{} is missing", command.id());
    }
}

/// `examples/sample.sh` uses every capability; running it here is how a capability with no
/// line in it is noticed (and how the script is kept from rotting).
#[test]
fn the_sample_script_runs() {
    let dir = Scratch::new("sample");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let out = Command::new("bash")
        .arg(root.join("examples/sample.sh"))
        .arg(&dir.0)
        .env("QUILL_BIN", BIN)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "examples/sample.sh failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = std::fs::read_to_string(dir.path("sample.txt")).unwrap();
    assert!(text.contains("Cut and pasted back: in a"), "{text}");
}

/// The script must call every subcommand at least once.
#[test]
fn the_sample_script_reaches_every_subcommand() {
    let script = include_str!("../../examples/sample.sh");
    let help = ok(&["--help"]);
    let commands = help
        .split_once("Commands:")
        .unwrap()
        .1
        .split_once("Options:")
        .unwrap()
        .0;
    for line in commands.lines() {
        let Some(name) = line.split_whitespace().next() else {
            continue;
        };
        if name == "help" {
            continue;
        }
        let used = script.contains(&format!("e {name} "))
            || script.contains(&format!("\" {name} "))
            || script.contains(&format!("e --format json {name} "))
            || script.contains(&format!("e {name} >"));
        assert!(
            used,
            "examples/sample.sh never runs `quill {name}` — add a line for it"
        );
    }
}
