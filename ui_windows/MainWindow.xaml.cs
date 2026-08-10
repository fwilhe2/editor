// The WinUI shell. Like the GTK one, it holds no editor state: the TextBox is a
// renderer for whatever `Viewport` returns, and every key is forwarded to the core
// through the UniFFI bindings.

using System;
using Microsoft.UI.Input;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using uniffi.editor_ffi;
using Windows.System;
using Windows.UI.Core;

// Windows.System also defines a DispatcherQueue; the WinUI one is the right one here.
using DispatcherQueue = Microsoft.UI.Dispatching.DispatcherQueue;

namespace EditorApp;

public sealed partial class MainWindow : Window
{
    private readonly EditorHandle _editor;
    private readonly DispatcherQueue _dispatcher;
    private bool _closing;

    public MainWindow(string path)
    {
        InitializeComponent();

        _dispatcher = DispatcherQueue.GetForCurrentThread();

        if (string.IsNullOrEmpty(path))
        {
            // No document to show; the other shells exit with usage here, but a GUI
            // has nowhere to print it, so start empty and say so.
            _editor = EditorHandle.Empty();
        }
        else
        {
            _editor = EditorHandle.Open(path);
        }

        _editor.SetListener(new WindowListener(this));

        // Resizing changes how many lines fit, which changes the viewport.
        TextArea.SizeChanged += (_, _) => Refresh();
        AppWindow.Closing += OnClosing;

        Refresh();
    }

    /// <summary>
    /// The foreign half of the observer contract. Rust may call this from any
    /// thread, so it hops onto the UI thread before touching widgets.
    /// </summary>
    private sealed class WindowListener : EditorListener
    {
        private readonly MainWindow _window;

        public WindowListener(MainWindow window) => _window = window;

        public void StateChanged() => _window._dispatcher.TryEnqueue(_window.Refresh);
    }

    private void Refresh()
    {
        var height = VisibleLines();
        var start = _editor.ScrollOffset();
        var viewport = _editor.Viewport(start, start + height);

        TextArea.Text = string.Join("\n", viewport.Lines);
        PlaceCaret(viewport);

        var dirty = _editor.IsDirty();
        SaveButton.IsEnabled = dirty;
        UndoButton.IsEnabled = _editor.CanUndo();
        RedoButton.IsEnabled = _editor.CanRedo();

        Title = (dirty ? "• " : string.Empty) + (_editor.Path() ?? "Untitled");
        StatusText.Text =
            $"Ln {viewport.Cursor.Line + 1}, Col {viewport.Cursor.Column + 1}    {viewport.TotalLines} lines";
    }

    /// Put the caret where the core says it is, as an offset into the rendered text.
    private void PlaceCaret(ViewportData viewport)
    {
        if (viewport.Cursor.Line < viewport.StartLine)
        {
            return;
        }

        var row = (int)(viewport.Cursor.Line - viewport.StartLine);
        var offset = 0;
        for (var i = 0; i < row && i < viewport.Lines.Length; i++)
        {
            offset += viewport.Lines[i].Length + 1; // + the newline joining them
        }

        offset += (int)viewport.Cursor.Column;
        TextArea.SelectionStart = Math.Min(offset, TextArea.Text.Length);
        TextArea.SelectionLength = 0;
    }

    /// <summary>
    /// How many lines fit. TextBox exposes no line height, so this approximates one
    /// from the font size — good enough to size the viewport, and the only number
    /// the core cannot work out for itself.
    /// </summary>
    private ulong VisibleLines()
    {
        var lineHeight = TextArea.FontSize * 1.4;
        var lines = (long)Math.Floor(TextArea.ActualHeight / lineHeight);
        return (ulong)Math.Max(1, lines);
    }

    private void OnKeyDown(object sender, KeyRoutedEventArgs e)
    {
        switch (e.Key)
        {
            case VirtualKey.Left:
                _editor.MoveCursor(MoveDirection.Left);
                break;
            case VirtualKey.Right:
                _editor.MoveCursor(MoveDirection.Right);
                break;
            case VirtualKey.Up:
                _editor.MoveCursor(MoveDirection.Up);
                break;
            case VirtualKey.Down:
                _editor.MoveCursor(MoveDirection.Down);
                break;
            case VirtualKey.Back:
                _editor.Backspace();
                break;
            case VirtualKey.Enter:
                _editor.InsertText("\n");
                break;
            case VirtualKey.Tab:
                _editor.InsertText("\t");
                break;
            default:
                return; // not ours; let the accelerators have it
        }

        e.Handled = true;
        _editor.FollowCursor(VisibleLines());
    }

    private void OnCharacterReceived(UIElement sender, CharacterReceivedRoutedEventArgs args)
    {
        // Ctrl+S must save, not type an 0x13 into the document.
        if (IsControlDown() || char.IsControl(args.Character))
        {
            return;
        }

        _editor.InsertText(args.Character.ToString());
        _editor.FollowCursor(VisibleLines());
        args.Handled = true;
    }

    private static bool IsControlDown() =>
        InputKeyboardSource
            .GetKeyStateForCurrentThread(VirtualKey.Control)
            .HasFlag(CoreVirtualKeyStates.Down);

    private void OnSave(object sender, RoutedEventArgs e) => Save();

    private void OnUndo(object sender, RoutedEventArgs e)
    {
        _editor.Undo();
        _editor.FollowCursor(VisibleLines());
    }

    private void OnRedo(object sender, RoutedEventArgs e)
    {
        _editor.Redo();
        _editor.FollowCursor(VisibleLines());
    }

    private bool Save()
    {
        try
        {
            _editor.Save();
            return true;
        }
        catch (EditorException error)
        {
            StatusText.Text = error.Message;
            return false;
        }
    }

    /// Windows convention: confirm before discarding unsaved work, Save as default.
    private async void OnClosing(AppWindow sender, AppWindowClosingEventArgs args)
    {
        if (_closing || !_editor.IsDirty())
        {
            return;
        }

        args.Cancel = true;

        var dialog = new ContentDialog
        {
            XamlRoot = Content.XamlRoot,
            Title = "Save changes?",
            Content = "Your changes will be lost if you don't save them.",
            PrimaryButtonText = "Save",
            SecondaryButtonText = "Discard",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Primary,
        };

        var choice = await dialog.ShowAsync();

        if (choice == ContentDialogResult.Primary && !Save())
        {
            return; // a failed save must not take the document down with it
        }

        if (choice == ContentDialogResult.None)
        {
            return; // cancelled
        }

        _closing = true;
        Close();
    }
}
