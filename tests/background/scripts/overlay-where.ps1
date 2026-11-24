# Part of Agent Record Adapter (see NOTICE). Ported from the desktop-recorder-mcp fork.
Add-Type -Namespace W -Name O -MemberDefinition @'
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern System.IntPtr FindWindowW(string c, string n);
[DllImport("user32.dll")] public static extern bool GetWindowRect(System.IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(System.IntPtr h);
[DllImport("user32.dll")] public static extern System.IntPtr GetForegroundWindow();
public struct RECT { public int L, T, R, B; }
'@
$h = [W.O]::FindWindowW("AgentDesktopCursorOverlay", [NullString]::Value)
if ($h -eq [IntPtr]::Zero) { "overlay: none"; exit }
$r = New-Object W.O+RECT; [void][W.O]::GetWindowRect($h, [ref]$r)
"overlay: visible=$([W.O]::IsWindowVisible($h)) rect=$($r.L),$($r.T),$($r.R),$($r.B)"
