// The macOS shell. Pure SwiftUI over the UniFFI bindings — the core reaches this
// process as a linked static library, not as a Cargo dependency.
//
// Apple HIG: the standard menu bar owns the shortcuts (⌘S, ⌘Z, ⇧⌘Z), the window
// gets a toolbar, and quitting with unsaved work asks first.

import AppKit
import SwiftUI

@main
struct EditorMacApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @StateObject private var model = EditorModel()

    var body: some Scene {
        WindowGroup {
            ContentView(model: model)
                .onAppear { delegate.model = model }
        }
        .commands {
            // Replace the stock items so the shortcuts drive the core rather than
            // AppKit's own undo stack, which knows nothing about our document.
            CommandGroup(replacing: .saveItem) {
                Button("Save") { model.save() }
                    .keyboardShortcut("s", modifiers: .command)
                    .disabled(!model.isDirty)
            }

            CommandGroup(replacing: .undoRedo) {
                Button("Undo") { model.undo() }
                    .keyboardShortcut("z", modifiers: .command)
                    .disabled(!model.canUndo)

                Button("Redo") { model.redo() }
                    .keyboardShortcut("z", modifiers: [.command, .shift])
                    .disabled(!model.canRedo)
            }
        }
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    weak var model: EditorModel?

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }

    /// HIG: never discard work silently. Save is the default button.
    func applicationShouldTerminate(
        _ sender: NSApplication
    ) -> NSApplication.TerminateReply {
        guard let model, model.isDirty else {
            return .terminateNow
        }

        let alert = NSAlert()
        alert.messageText = "Save changes before quitting?"
        alert.informativeText = "Your changes will be lost if you don't save them."
        alert.addButton(withTitle: "Save")
        alert.addButton(withTitle: "Discard")
        alert.addButton(withTitle: "Cancel")

        switch alert.runModal() {
        case .alertFirstButtonReturn:
            // A failed save must not take the document down with it.
            return model.save() ? .terminateNow : .terminateCancel
        case .alertSecondButtonReturn:
            return .terminateNow
        default:
            return .terminateCancel
        }
    }
}
