// Renders the viewport and forwards keys. Draws no text it did not get from the
// core, and keeps no copy of the document.

import EditorCore
import SwiftUI

struct ContentView: View {
    @ObservedObject var model: EditorModel

    /// Matches `.body` in a monospaced design closely enough to size the viewport.
    private let lineHeight: CGFloat = 18

    var body: some View {
        VStack(spacing: 0) {
            GeometryReader { geometry in
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(model.viewport.lines.enumerated()), id: \.offset) { index, line in
                        row(index: index, line: line)
                    }
                    Spacer(minLength: 0)
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                .contentShape(Rectangle())
                .onAppear { report(height: geometry.size.height) }
                .onChange(of: geometry.size.height) { _, height in report(height: height) }
            }
            .font(.system(.body, design: .monospaced))
            .focusable()
            .focusEffectDisabled()
            .onKeyPress(phases: .down, action: handle)

            Divider()
            statusBar
        }
        .frame(minWidth: 480, minHeight: 320)
        .navigationTitle(model.documentName)
        .toolbar {
            ToolbarItemGroup {
                Button(action: model.undo) {
                    Label("Undo", systemImage: "arrow.uturn.backward")
                }
                .disabled(!model.canUndo)

                Button(action: model.redo) {
                    Label("Redo", systemImage: "arrow.uturn.forward")
                }
                .disabled(!model.canRedo)

                Button { model.save() } label: {
                    Label("Save", systemImage: "square.and.arrow.down")
                }
                .disabled(!model.isDirty)
            }
        }
    }

    /// One line, with the caret drawn into it when the cursor is on this row.
    @ViewBuilder
    private func row(index: Int, line: String) -> some View {
        let cursor = model.viewport.cursor
        let isCursorRow = UInt64(index) + model.viewport.startLine == cursor.line

        if isCursorRow {
            let split = line.index(
                line.startIndex,
                offsetBy: min(Int(cursor.column), line.count)
            )
            HStack(spacing: 0) {
                Text(String(line[line.startIndex..<split]))
                Rectangle()
                    .frame(width: 1.5, height: lineHeight)
                    .foregroundStyle(.tint)
                Text(String(line[split...]))
                Spacer(minLength: 0)
            }
            .frame(height: lineHeight)
        } else {
            // A space keeps blank lines from collapsing to zero height.
            Text(line.isEmpty ? " " : line)
                .frame(height: lineHeight, alignment: .leading)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private var statusBar: some View {
        HStack {
            Text(
                "Ln \(model.viewport.cursor.line + 1), Col \(model.viewport.cursor.column + 1)"
            )
            Text("\(model.viewport.totalLines) lines")
            if model.isDirty {
                Text("Edited")
            }
            Spacer()
            Text(model.status)
        }
        .font(.caption)
        .foregroundStyle(.secondary)
        .padding(.horizontal, 12)
        .padding(.vertical, 4)
    }

    private func report(height: CGFloat) {
        let lines = max(1, Int((height - 16) / lineHeight))
        model.viewportHeightChanged(to: UInt64(lines))
    }

    private func handle(_ press: KeyPress) -> KeyPress.Result {
        // ⌘-shortcuts belong to the menu bar, not the document.
        if press.modifiers.contains(.command) {
            return .ignored
        }

        switch press.key {
        case .leftArrow: model.move(.left)
        case .rightArrow: model.move(.right)
        case .upArrow: model.move(.up)
        case .downArrow: model.move(.down)
        case .delete: model.backspace()
        case .return: model.insert("\n")
        case .tab: model.insert("\t")
        default:
            let text = press.characters.filter { !$0.unicodeScalars.contains { scalar in
                CharacterSet.controlCharacters.contains(scalar)
            } }
            guard !text.isEmpty else { return .ignored }
            model.insert(text)
        }

        return .handled
    }
}
