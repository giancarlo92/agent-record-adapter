# Part of Agent Record Adapter (see NOTICE).
# The foreground thread's GUI state: active, focus and capture windows, and
# the modes it is in (menu, move/size, capture). For debugging lost clicks.
Add-Type -Namespace W -Name Gui -MemberDefinition @'
[StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
[StructLayout(LayoutKind.Sequential)] public struct GUITHREADINFO {
  public int cbSize; public int flags; public System.IntPtr hwndActive; public System.IntPtr hwndFocus;
  public System.IntPtr hwndCapture; public System.IntPtr hwndMenuOwner; public System.IntPtr hwndMoveSize;
  public System.IntPtr hwndCaret; public RECT rcCaret; }
[DllImport("user32.dll")] public static extern bool GetGUIThreadInfo(uint thread, ref GUITHREADINFO info);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(System.IntPtr h, out uint p);
'@
$info = New-Object W.Gui+GUITHREADINFO
$info.cbSize = [System.Runtime.InteropServices.Marshal]::SizeOf($info)
[void][W.Gui]::GetGUIThreadInfo(0, [ref]$info)
function Owner($h) { if ($h -eq [IntPtr]::Zero) { return '-' }; $p = 0; [void][W.Gui]::GetWindowThreadProcessId($h, [ref]$p); "w-$h (" + (Get-Process -Id $p -ErrorAction SilentlyContinue).ProcessName + ")" }
"flags=0x{0:X} active={1} focus={2} capture={3}" -f $info.flags, (Owner $info.hwndActive), (Owner $info.hwndFocus), (Owner $info.hwndCapture)
