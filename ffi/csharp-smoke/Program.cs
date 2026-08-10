// Drives the editor core from C# through the UniFFI bindings and asserts the
// results. Any failure exits non-zero so CI notices.
//
// This is the only place the FFI layer is checked end to end. The WinUI shell
// cannot be run headlessly, so if this passes on Windows the boundary is sound
// and anything still broken is XAML.

using System;
using System.IO;
using System.Linq;
using uniffi.editor_ffi;

namespace EditorFfi.Smoke;

/// The foreign half of the observer contract: Rust calls this.
internal sealed class CountingListener : EditorListener
{
    public int Count;

    public void StateChanged() => Count++;
}

internal static class Program
{
    private static int _failures;

    private static int Main()
    {
        var path = Path.Combine(Path.GetTempPath(), $"editor-ffi-smoke-{Environment.ProcessId}.txt");
        File.WriteAllText(path, "beta\n");

        try
        {
            var editor = EditorHandle.Open(path);
            var listener = new CountingListener();
            editor.SetListener(listener);

            // Editing reaches the core.
            editor.SetCursor(0, 0);
            editor.InsertText("alpha\n");
            Check("insert reaches the core", editor.CharCount() == 11);
            Check("the cursor advanced", editor.Cursor().Line == 1 && editor.Cursor().Column == 0);
            Check("the document is dirty", editor.IsDirty());

            // Rust called back into C#.
            Check("the listener was notified", listener.Count >= 2);

            // The viewport crosses the boundary intact.
            var viewport = editor.Viewport(0, 10);
            Check("viewport lines", viewport.Lines.Take(2).SequenceEqual(new[] { "alpha", "beta" }));
            Check("viewport total", viewport.TotalLines == 3);

            // Saving writes the real bytes.
            editor.Save();
            Check("saved to disk", File.ReadAllText(path) == "alpha\nbeta\n");
            Check("no longer dirty", !editor.IsDirty());

            // History survives the boundary.
            Check("can undo", editor.CanUndo());
            editor.Undo();
            Check("undo removed the insert", editor.Viewport(0, 10).Lines[0] == "beta");
            editor.Redo();
            Check("redo restored it", editor.Viewport(0, 10).Lines[0] == "alpha");

            // Scrolling is the core's, not the shell's.
            editor.SetCursor(2, 0);
            Check("follow_cursor scrolls", editor.FollowCursor(1) == 2);

            // Errors arrive as exceptions rather than silent failure.
            try
            {
                EditorHandle.Open(Path.Combine(Path.GetTempPath(), "definitely-missing-file"));
                Check("a missing file throws", false);
            }
            catch (EditorException)
            {
                Check("a missing file throws", true);
            }
        }
        finally
        {
            File.Delete(path);
        }

        Console.WriteLine(_failures == 0 ? "FFI smoke test passed" : $"{_failures} check(s) failed");
        return _failures == 0 ? 0 : 1;
    }

    private static void Check(string what, bool ok)
    {
        Console.WriteLine($"{(ok ? "ok  " : "FAIL")}  {what}");
        if (!ok)
        {
            _failures++;
        }
    }
}
