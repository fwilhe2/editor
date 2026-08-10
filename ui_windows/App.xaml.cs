using System;
using Microsoft.UI.Xaml;

namespace EditorApp;

public partial class App : Application
{
    private Window _window;

    public App()
    {
        InitializeComponent();
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        // Unpackaged, so the file argument arrives on the plain command line.
        var argv = Environment.GetCommandLineArgs();
        var path = argv.Length > 1 ? argv[1] : null;

        _window = new MainWindow(path);
        _window.Activate();
    }
}
