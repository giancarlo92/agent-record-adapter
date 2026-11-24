# Part of Agent Record Adapter (see NOTICE).
# A small test app with a classic Win32 menu bar (WinForms MainMenu is a real
# HMENU), a text box and a push button, for `menu`, shortcuts run as menu
# commands and the BN_CLICKED click route. It shows what happened in a label.
# Run: powershell -NoProfile -ExecutionPolicy Bypass -File classic-menu.ps1
Add-Type -AssemblyName System.Windows.Forms
$form = New-Object System.Windows.Forms.Form
$form.Text = 'Classic menu test'
$form.Width = 420
$form.Height = 220

$status = New-Object System.Windows.Forms.Label
$status.Name = 'status'
$status.Text = 'status: start'
$status.AutoSize = $true
$status.Top = 110
$status.Left = 10
$form.Controls.Add($status)

$box = New-Object System.Windows.Forms.TextBox
$box.Name = 'entry'
$box.Top = 20
$box.Left = 10
$box.Width = 380
$form.Controls.Add($box)

$button = New-Object System.Windows.Forms.Button
$button.Text = 'Apply'
$button.Top = 60
$button.Left = 10
$button.Add_Click({ $status.Text = 'status: applied ' + $box.Text })
$form.Controls.Add($button)

$menu = New-Object System.Windows.Forms.MainMenu
$file = $menu.MenuItems.Add('&File')
$save = New-Object System.Windows.Forms.MenuItem('&Save', { $status.Text = 'status: saved' })
$save.Shortcut = [System.Windows.Forms.Shortcut]::CtrlS
[void]$file.MenuItems.Add($save)
$export = $file.MenuItems.Add('E&xport')
[void]$export.MenuItems.Add('As &CSV...', { $status.Text = 'status: exported csv' })
$edit = $menu.MenuItems.Add('&Edit')
[void]$edit.MenuItems.Add('&Clear', { $box.Text = ''; $status.Text = 'status: cleared' })
$form.Menu = $menu

[void]$form.ShowDialog()
