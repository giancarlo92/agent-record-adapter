# Part of Agent Record Adapter (see NOTICE). Ported from the desktop-recorder-mcp fork.
Get-CimInstance Win32_Process -Filter "Name='agent-desktop.exe'" | ForEach-Object { "{0} parent={1} {2}" -f $_.ProcessId, $_.ParentProcessId, ($_.CommandLine -replace '^"[^"]*"\s*','') }
