// Drives the editor core from Swift through the UniFFI bindings and asserts the
// results, with no UI involved.
//
// The mirror of ffi/csharp-smoke: because it is a plain executable it runs on
// Linux against libeditor_ffi.so as well as on macOS, which is what makes the
// Swift boundary verifiable without a Mac. If this passes and the app misbehaves,
// the bug is in SwiftUI, not the bindings.

import EditorCore
import Foundation

final class CountingListener: EditorListener, @unchecked Sendable {
    private(set) var count = 0

    func stateChanged() {
        count += 1
    }
}

var failures = 0

func check(_ what: String, _ ok: Bool) {
    print("\(ok ? "ok  " : "FAIL")  \(what)")
    if !ok { failures += 1 }
}

let path = NSTemporaryDirectory() + "editor-swift-smoke-\(ProcessInfo.processInfo.processIdentifier).txt"
try "beta\n".write(toFile: path, atomically: true, encoding: .utf8)
defer { try? FileManager.default.removeItem(atPath: path) }

let editor = try EditorHandle.open(path: path)
let listener = CountingListener()
editor.setListener(listener: listener)

// Editing reaches the core.
_ = editor.setCursor(line: 0, column: 0)
editor.insertText(text: "alpha\n")
check("insert reaches the core", editor.charCount() == 11)
check("the cursor advanced", editor.cursor() == CursorPosition(line: 1, column: 0))
check("the document is dirty", editor.isDirty())

// Rust called back into Swift.
check("the listener was notified", listener.count >= 2)

// The viewport crosses the boundary intact.
let viewport = editor.viewport(startLine: 0, endLine: 10)
check("viewport lines", Array(viewport.lines.prefix(2)) == ["alpha", "beta"])
check("viewport total", viewport.totalLines == 3)

// Saving writes the real bytes.
try editor.save()
check("saved to disk", try String(contentsOfFile: path, encoding: .utf8) == "alpha\nbeta\n")
check("no longer dirty", !editor.isDirty())

// History survives the boundary.
check("can undo", editor.canUndo())
_ = editor.undo()
check("undo removed the insert", editor.viewport(startLine: 0, endLine: 10).lines[0] == "beta")
_ = editor.redo()
check("redo restored it", editor.viewport(startLine: 0, endLine: 10).lines[0] == "alpha")

// Scrolling is the core's, not the shell's.
_ = editor.setCursor(line: 2, column: 0)
check("followCursor scrolls", editor.followCursor(height: 1) == 2)

// Errors arrive as thrown Swift errors rather than silent failure.
do {
    _ = try EditorHandle.open(path: NSTemporaryDirectory() + "definitely-missing-file")
    check("a missing file throws", false)
} catch is EditorError {
    check("a missing file throws", true)
}

print(failures == 0 ? "FFI smoke test passed" : "\(failures) check(s) failed")
exit(failures == 0 ? 0 : 1)
