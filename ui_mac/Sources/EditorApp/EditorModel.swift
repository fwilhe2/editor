// Everything the views read, derived from the core each refresh.
//
// The model holds no document state of its own: `viewport`, `isDirty` and the
// rest are snapshots of what the core just reported, republished so SwiftUI can
// observe them.

import EditorCore
import SwiftUI

@MainActor
final class EditorModel: ObservableObject {
    @Published private(set) var viewport: ViewportData
    @Published private(set) var isDirty = false
    @Published private(set) var canUndo = false
    @Published private(set) var canRedo = false
    @Published private(set) var documentName = "Untitled"
    @Published private(set) var status = ""

    private let handle: EditorHandle
    private var listener: RefreshListener?

    /// How many lines the window currently shows. The one number the core cannot
    /// work out for itself, so the view reports it after every layout.
    private var visibleLines: UInt64 = 40

    init() {
        // Launched from a terminal with a path; launched from Finder without one.
        let path = CommandLine.arguments.dropFirst().first { !$0.hasPrefix("-") }
        var openError: String?

        if let path {
            do {
                handle = try EditorHandle.open(path: path)
            } catch {
                handle = EditorHandle.empty()
                openError = "\(error)"
            }
        } else {
            handle = EditorHandle.empty()
        }

        viewport = handle.viewport(startLine: 0, endLine: visibleLines)
        status = openError ?? ""

        let listener = RefreshListener()
        handle.setListener(listener: listener)
        self.listener = listener
        listener.model = self

        refresh()
    }

    func refresh() {
        let start = handle.scrollOffset()
        viewport = handle.viewport(startLine: start, endLine: start + visibleLines)
        isDirty = handle.isDirty()
        canUndo = handle.canUndo()
        canRedo = handle.canRedo()
        documentName =
            handle.path().map { URL(fileURLWithPath: $0).lastPathComponent } ?? "Untitled"
    }

    func viewportHeightChanged(to lines: UInt64) {
        guard lines != visibleLines, lines > 0 else { return }
        visibleLines = lines
        refresh()
    }

    // --- editing --------------------------------------------------------------

    func insert(_ text: String) {
        handle.insertText(text: text)
        settle()
    }

    func backspace() {
        _ = handle.backspace()
        settle()
    }

    func move(_ direction: MoveDirection) {
        handle.moveCursor(direction: direction)
        settle()
    }

    func undo() {
        status = handle.undo() ? "" : "Nothing to undo"
        settle()
    }

    func redo() {
        status = handle.redo() ? "" : "Nothing to redo"
        settle()
    }

    @discardableResult
    func save() -> Bool {
        do {
            try handle.save()
            status = "Saved"
            refresh()
            return true
        } catch {
            status = "\(error)"
            refresh()
            return false
        }
    }

    /// Keep the caret on screen, then repaint.
    private func settle() {
        _ = handle.followCursor(height: visibleLines)
        refresh()
    }
}

/// The foreign half of the observer contract: Rust calls this when the document
/// changes. It may arrive on any thread, so it hops to the main actor.
final class RefreshListener: EditorListener, @unchecked Sendable {
    weak var model: EditorModel?

    func stateChanged() {
        Task { @MainActor [weak self] in
            self?.model?.refresh()
        }
    }
}
