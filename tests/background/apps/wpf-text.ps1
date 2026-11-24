# Part of Agent Record Adapter (see NOTICE).
# A small WPF window with one text box, to check keys and typed text in a WPF app whose window is
# behind or deactivated. Its title shows the box's text, so a test reads it from the window list.
# Run: powershell -NoProfile -STA -ExecutionPolicy Bypass -File wpf-text.ps1
Add-Type -AssemblyName PresentationFramework
$window = New-Object System.Windows.Window
$window.Title = 'WPF text test'
$window.Width = 420
$window.Height = 160
$box = New-Object System.Windows.Controls.TextBox
$box.Name = 'entry'
$box.Margin = '12'
$box.Add_TextChanged({ $window.Title = 'WPF text test: ' + $box.Text })
$window.Content = $box
$window.Add_ContentRendered({ [void]$box.Focus() })
[void]$window.ShowDialog()
