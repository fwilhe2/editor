//! End-to-end tests for the `edit` binary.
//!
//! These cover the contract scripts and agents depend on: text is written before the
//! process exits, `--session` carries cursor and history across invocations, stdout
//! stays parseable, and failures exit non-zero.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Sandbox {
    dir: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("editor-cli-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Sandbox { dir }
    }

    fn file(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn edit(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_edit"))
        .args(args)
        .output()
        .expect("the edit binary runs")
}

fn ok(args: &[&str]) -> String {
    let output = edit(args);
    assert!(
        output.status.success(),
        "expected success from {args:?}, got {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn json(args: &[&str]) -> serde_json::Value {
    serde_json::from_str(&ok(args)).expect("stdout is a single JSON object")
}

fn contents(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

fn s(path: &Path) -> String {
    path.display().to_string()
}

#[test]
fn insert_writes_the_document_before_exiting() {
    let sandbox = Sandbox::new("insert");
    let file = sandbox.file("doc.txt", "hello\n");

    ok(&["insert", &s(&file), "--text", "brave ", "--line", "1", "--col", "1"]);
    assert_eq!(contents(&file), "brave hello\n");
}

#[test]
fn addressing_is_one_based() {
    let sandbox = Sandbox::new("one-based");
    let file = sandbox.file("doc.txt", "ab\ncd\n");

    // Line 2, column 2 is between 'c' and 'd'.
    ok(&["insert", &s(&file), "--text", "X", "--line", "2", "--col", "2"]);
    assert_eq!(contents(&file), "ab\ncXd\n");
}

#[test]
fn view_prints_bare_lines_and_clamps_the_range() {
    let sandbox = Sandbox::new("view");
    let file = sandbox.file("doc.txt", "one\ntwo\nthree\n");

    assert_eq!(ok(&["view", &s(&file), "--start", "2", "--lines", "2"]), "two\nthree\n");

    let report = json(&["--format", "json", "view", &s(&file)]);
    assert_eq!(report["start_line"], 1);
    assert_eq!(report["total_lines"], 4); // trailing newline opens a final empty line
    assert_eq!(report["lines"][0], "one");
}

#[test]
fn a_session_carries_the_cursor_between_invocations() {
    let sandbox = Sandbox::new("session-cursor");
    let file = sandbox.file("doc.txt", "abc\n");
    let session = sandbox.path("s.json");

    ok(&["--session", &s(&session), "move", &s(&file), "right", "--times", "2"]);
    let report = json(&["--format", "json", "--session", &s(&session), "info", &s(&file)]);
    assert_eq!(report["cursor"]["column"], 3);

    ok(&["--session", &s(&session), "insert", &s(&file), "--text", "-X-"]);
    assert_eq!(contents(&file), "ab-X-c\n");
}

#[test]
fn undo_and_redo_travel_through_the_session() {
    let sandbox = Sandbox::new("undo");
    let file = sandbox.file("doc.txt", "base\n");
    let session = sandbox.path("s.json");

    let (session, doc) = (s(&session), s(&file));

    ok(&[
        "--session", &session, "insert", &doc, "--text", "X", "--line", "1", "--col", "1",
    ]);
    assert_eq!(contents(&file), "Xbase\n");

    ok(&["--session", &session, "undo", &doc]);
    assert_eq!(contents(&file), "base\n");

    ok(&["--session", &session, "redo", &doc]);
    assert_eq!(contents(&file), "Xbase\n");
}

#[test]
fn undo_without_a_session_fails_loudly() {
    let sandbox = Sandbox::new("undo-no-session");
    let file = sandbox.file("doc.txt", "base\n");

    let output = edit(&["undo", &s(&file)]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "errors must not reach stdout");
    assert!(String::from_utf8_lossy(&output.stderr).contains("--session"));
}

#[test]
fn a_no_op_undo_reports_no_change_and_still_succeeds() {
    let sandbox = Sandbox::new("undo-empty");
    let file = sandbox.file("doc.txt", "base\n");
    let session = sandbox.path("s.json");

    let report = json(&["--format", "json", "--session", &s(&session), "undo", &s(&file)]);
    assert_eq!(report["changed"], false);
    assert_eq!(report["written"], false);
    assert_eq!(contents(&file), "base\n");
}

#[test]
fn backspace_repeats_and_stops_at_the_document_start() {
    let sandbox = Sandbox::new("backspace");
    let file = sandbox.file("doc.txt", "abcdef");

    ok(&["backspace", &s(&file), "--line", "1", "--col", "4", "--times", "10"]);
    assert_eq!(contents(&file), "def");
}

#[test]
fn dry_run_reports_the_result_without_touching_disk() {
    let sandbox = Sandbox::new("dry-run");
    let file = sandbox.file("doc.txt", "keep\n");
    let session = sandbox.path("s.json");

    let report = json(&[
        "--format", "json", "--dry-run", "--session", &s(&session),
        "insert", &s(&file), "--text", "X", "--line", "1", "--col", "1",
    ]);
    assert_eq!(report["changed"], true);
    assert_eq!(report["written"], false);
    assert_eq!(contents(&file), "keep\n");
    assert!(!session.exists(), "a dry run writes no session either");
}

#[test]
fn text_can_come_from_stdin() {
    let sandbox = Sandbox::new("stdin");
    let file = sandbox.file("doc.txt", "");

    let mut child = Command::new(env!("CARGO_BIN_EXE_edit"))
        .args(["insert", &s(&file), "--text", "-"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        child.stdin.as_mut().unwrap().write_all(b"piped in").unwrap();
    }
    assert!(child.wait().unwrap().success());
    assert_eq!(contents(&file), "piped in");
}

#[test]
fn new_creates_a_file_and_refuses_to_clobber_one() {
    let sandbox = Sandbox::new("new");
    let file = sandbox.path("fresh.txt");

    ok(&["new", &s(&file)]);
    assert_eq!(contents(&file), "");

    std::fs::write(&file, "precious").unwrap();
    let output = edit(&["new", &s(&file)]);
    assert!(!output.status.success());
    assert_eq!(contents(&file), "precious");

    ok(&["new", &s(&file), "--force"]);
    assert_eq!(contents(&file), "");
}

#[test]
fn a_missing_file_is_an_error_not_an_empty_document() {
    let sandbox = Sandbox::new("missing");
    let output = edit(&["view", &s(&sandbox.path("nope.txt"))]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("nope.txt"));
}
