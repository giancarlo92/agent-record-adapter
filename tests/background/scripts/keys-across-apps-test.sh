#!/usr/bin/env bash
# Part of Agent Record Adapter (see NOTICE).
# Typed text and keys in a deactivated window, across UI technologies, on the hidden work desktop:
# WinForms (classic-menu.ps1) and WPF (wpf-text.ps1); WordCraft (winit) is wordcraft-keys-test.sh. Each app
# gets the focus on its field, then a short-lived window takes the activation and goes away (as
# the person's windows take it on theirs), then: type "abc", Backspace, type "xyz" (a key every
# text box takes; the old WinForms text box has no Ctrl+A). The field must read "abxyz".
# Only the processes this test started are closed at the end (by the pid launch reports).
# Usage: keys-across-apps-test.sh <agent-record.exe>
set -u
AD="$1"
export AGENT_DESKTOP_WORK_DESKTOP=artest AGENT_DESKTOP_HOME="$LOCALAPPDATA/DesktopRecorder/tmp/js-state"
APPS="$(cd "$(dirname "$0")/../apps" && pwd)"
r() { node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);process.stdout.write(o.ok?'ok':'ERR('+o.error.message.slice(0,60)+')')})"; }
deactivate() { "$AD" launch "C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe" --arg=-NoProfile --arg=-Command --arg="Start-Sleep -Seconds 2" --timeout 30000 >/dev/null 2>&1; sleep 4; }
win() { "$AD" list-windows --app "$1" | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const w=o.data.find(w=>new RegExp(process.argv[1]).test(w.title));console.log(w?w.id+' '+(w.is_focused?'active':'deactivated'):'')})" "$2"; }
field() { "$AD" snapshot --window-id "$1" | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);let h='';(function w(n){if(!n||h)return;if(['textfield','document'].includes(n.role)&&n.ref_id&&new RegExp(process.argv[1]).test(n.name||'')){h=n.ref_id};(n.children||[]).forEach(w)})(o.data&&o.data.tree);console.log(h)})" "$2"; }
value() { "$AD" snapshot --window-id "$1" | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);let h=null;(function w(n){if(!n||h!==null)return;if(['textfield','document'].includes(n.role)&&new RegExp(process.argv[1]).test(n.name||''))h=n.value||'';(n.children||[]).forEach(w)})(o.data&&o.data.tree);console.log(JSON.stringify((h||'').trim()))})" "$2"; }
check() { # name, app exe, window title regex, field name regex
  local NAME="$1" EXE="$2" TITLE="$3" FIELD="$4" W ST REF
  read W ST < <(win "$EXE" "$TITLE")
  REF=$(field "$W" "$FIELD")
  [ -n "$REF" ] && "$AD" focus "$REF" >/dev/null 2>&1
  deactivate
  read W ST < <(win "$EXE" "$TITLE")
  printf "%-10s window %s: type " "$NAME" "$ST"; "$AD" type "$W" "abc" | r
  printf ", backspace "; "$AD" press backspace --window-id "$W" | r
  printf ", type "; "$AD" type "$W" "xyz" | r
  local V; V=$(value "$W" "$FIELD")
  [ "$V" = '"abxyz"' ] && echo " -> PASS ($V)" || echo " -> FAIL (field: $V)"
}
PIDS=""
# launch writes to a file: a pipe would stay open as long as the launched app runs.
OUTF="$LOCALAPPDATA/DesktopRecorder/tmp/keys-launch.json"
started() { node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{try{const o=JSON.parse(s);console.log((o.data&&o.data.pid)||'')}catch(e){console.log('')}})"; }
cleanup() { for p in $PIDS; do powershell -NoProfile -Command "Stop-Process -Id $p -Force -ErrorAction SilentlyContinue"; done; }
"$AD" launch "C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe" --arg=-NoProfile --arg=-ExecutionPolicy --arg=Bypass --arg=-File "--arg=$(cygpath -w "$APPS/classic-menu.ps1")" --timeout 30000 >"$OUTF" 2>/dev/null; P=$(started <"$OUTF"); PIDS="$PIDS $P"; sleep 4
check WinForms powershell.exe 'Classic menu test' '.*'
"$AD" launch "C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe" --arg=-NoProfile --arg=-STA --arg=-ExecutionPolicy --arg=Bypass --arg=-File "--arg=$(cygpath -w "$APPS/wpf-text.ps1")" --timeout 30000 >"$OUTF" 2>/dev/null; P=$(started <"$OUTF"); PIDS="$PIDS $P"; sleep 5
check WPF powershell.exe 'WPF text test' '.*'
cleanup
