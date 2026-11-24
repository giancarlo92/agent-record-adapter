# Part of Agent Record Adapter (see NOTICE). Ported from the desktop-recorder-mcp fork.
# The window in front and its process name, to check an action left it where it was.
Add-Type -Namespace W -Name Fg -MemberDefinition @'
[DllImport("user32.dll")] public static extern System.IntPtr GetForegroundWindow();
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(System.IntPtr h, out uint p);
'@
$h = [W.Fg]::GetForegroundWindow()
$p = [uint32]0
[void][W.Fg]::GetWindowThreadProcessId($h, [ref]$p)
$name = try { (Get-Process -Id $p -ErrorAction Stop).ProcessName } catch { '?' }
"$h $name"
