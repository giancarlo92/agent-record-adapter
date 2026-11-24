# Part of Agent Record Adapter (see NOTICE).
# Builds the WinForms and WPF test apps as programs of their own (classic-menu.exe, wpf-text.exe),
# the same windows as classic-menu.ps1 and wpf-text.ps1. A test launching powershell.exe on the
# person's desktop attaches to a PowerShell already running there (upstream's launch rule); a
# program of its own always starts fresh. Built with the .NET Framework compiler that ships with
# Windows, into the given folder (default: %LOCALAPPDATA%\DesktopRecorder\tmp\test-apps).
# Run: powershell -NoProfile -ExecutionPolicy Bypass -File build-test-apps.ps1 [-Out <folder>]
param([string]$Out = (Join-Path $env:LOCALAPPDATA 'DesktopRecorder\tmp\test-apps'))
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $Out | Out-Null
$at = { param($name) [System.Reflection.Assembly]::LoadWithPartialName($name).Location }

$winforms = @'
using System;
using System.Windows.Forms;
public static class ClassicMenuTest {
    [STAThread]
    public static void Main() {
        var form = new Form { Text = "Classic menu test", Width = 420, Height = 220 };
        var status = new Label { Name = "status", Text = "status: start", AutoSize = true, Top = 110, Left = 10 };
        var box = new TextBox { Name = "entry", Top = 20, Left = 10, Width = 380 };
        var button = new Button { Text = "Apply", Top = 60, Left = 10 };
        button.Click += (s, e) => status.Text = "status: applied " + box.Text;
        form.Controls.Add(status); form.Controls.Add(box); form.Controls.Add(button);
        var menu = new MainMenu();
        var file = menu.MenuItems.Add("&File");
        var save = new MenuItem("&Save", (s, e) => status.Text = "status: saved") { Shortcut = Shortcut.CtrlS };
        file.MenuItems.Add(save);
        var export = file.MenuItems.Add("E&xport");
        export.MenuItems.Add("As &CSV...", (s, e) => status.Text = "status: exported csv");
        var edit = menu.MenuItems.Add("&Edit");
        edit.MenuItems.Add("&Clear", (s, e) => { box.Text = ""; status.Text = "status: cleared"; });
        form.Menu = menu;
        Application.Run(form);
    }
}
'@
Add-Type -TypeDefinition $winforms -ReferencedAssemblies System.Windows.Forms, System.Drawing `
    -OutputAssembly (Join-Path $Out 'classic-menu.exe') -OutputType WindowsApplication

$wpfText = @'
using System;
using System.Windows;
using System.Windows.Controls;
public static class WpfTextTest {
    [STAThread]
    public static void Main() {
        var window = new Window { Title = "WPF text test", Width = 420, Height = 160 };
        var box = new TextBox { Name = "entry", Margin = new Thickness(12) };
        box.TextChanged += (s, e) => window.Title = "WPF text test: " + box.Text;
        window.Content = box;
        window.ContentRendered += (s, e) => box.Focus();
        new Application().Run(window);
    }
}
'@
Add-Type -TypeDefinition $wpfText -OutputAssembly (Join-Path $Out 'wpf-text.exe') -OutputType WindowsApplication `
    -ReferencedAssemblies (& $at PresentationFramework), (& $at PresentationCore), (& $at WindowsBase), (& $at System.Xaml)
Write-Output $Out
