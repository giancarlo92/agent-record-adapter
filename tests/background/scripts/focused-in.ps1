# Part of Agent Record Adapter (see NOTICE). Ported from the desktop-recorder-mcp fork.
param([long]$Hwnd)
# The element that has keyboard focus inside a (possibly background) window, per UI Automation.
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$root = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$Hwnd)
$cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::HasKeyboardFocusProperty, $true)
$sw = [Diagnostics.Stopwatch]::StartNew()
$found = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
$ms = $sw.ElapsedMilliseconds
if ($found) {
  $c = $found.Current
  "focused: {0} '{1}' ({2} ms)" -f $c.ControlType.ProgrammaticName, $c.Name, $ms
} else { "focused: none ($ms ms)" }
$sys = [System.Windows.Automation.AutomationElement]::FocusedElement
"system focus pid: {0}" -f $sys.Current.ProcessId
