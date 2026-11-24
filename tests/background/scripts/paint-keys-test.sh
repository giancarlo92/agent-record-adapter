#!/usr/bin/env bash
# Part of Agent Record Adapter (see NOTICE).
# Windows Paint (a WinUI app) in the background, its window deactivated, on the hidden work desktop:
# Ctrl+PageUp zooms in (the Zoom slider leaves 100), Ctrl+E opens Image Properties, the width typed
# into its field and Return apply it (Image Properties shows it again), Escape closes it, and a
# click on the Pencil and Text tools (WinUI toggle buttons) selects them (the status says so).
# A click or a stroke on Paint's canvas takes only real pointer input: it needs --headed.
# With "person" as second argument it runs on the person's own desktop, Paint opening behind their
# windows, and the window in front must stay the same.
# Paint may reuse a process already running: only a process this test started is closed.
# Usage: paint-keys-test.sh <agent-record.exe> [person]
set -u
AD="$1"; WHERE="${2:-hidden}"
export AGENT_DESKTOP_HOME="$LOCALAPPDATA/DesktopRecorder/tmp/js-state"
[ "$WHERE" = person ] || export AGENT_DESKTOP_WORK_DESKTOP=artest
HERE="$(cd "$(dirname "$0")" && pwd)"
FG() { powershell -NoProfile -ExecutionPolicy Bypass -File "$HERE/foreground.ps1"; }
BEFORE=$(FG)
PS="C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"
EARLIER=$(powershell -NoProfile -Command "(Get-Process mspaint -ErrorAction SilentlyContinue).Id -join ' '")
OUTF="$LOCALAPPDATA/DesktopRecorder/tmp/paint-launch.json"
"$AD" launch "$(cygpath -w "$LOCALAPPDATA/Microsoft/WindowsApps/mspaint.exe")" --timeout 30000 >"$OUTF" 2>&1
PID=$(node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{try{console.log(JSON.parse(s).data.pid)}catch(e){console.log('')}})" <"$OUTF")
sleep 5
W=$("$AD" list-windows --app mspaint.exe | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const w=o.data.find(w=>w.pid==process.argv[1]&&/Paint/.test(w.title));console.log(w?w.id:'')})" "$PID")
[ "$WHERE" = person ] || { "$AD" launch "$PS" --arg=-NoProfile --arg=-Command --arg="Start-Sleep -Seconds 2" --timeout 30000 >/dev/null 2>&1; sleep 4; }
ST=$("$AD" list-windows --app mspaint.exe | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const w=o.data.find(w=>w.id===process.argv[1]);console.log(w&&w.is_focused?'active':'deactivated')})" "$W")
echo "window $W ($ST)"
tree() { "$AD" snapshot --window-id "$W" | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const out=[];(function w(n){if(!n)return;out.push((n.ref_id||'-')+'\t'+(n.role||'')+'\t'+(n.name||'')+'\t'+(n.value||''));(n.children||[]).forEach(w)})(o.data&&o.data.tree);console.log(out.join('\n'))})"; }
field() { tree | awk -F'\t' -v role="$1" -v name="$2" '$2==role && $3 ~ name {print $1"\t"$4; exit}'; }
r() { node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);process.stdout.write(o.ok?'ok':'ERR('+o.error.message.slice(0,70)+')')})"; }
PASS=0; FAIL=0
check() { if [ "$1" = yes ]; then PASS=$((PASS+1)); echo "    -> PASS ($2)"; else FAIL=$((FAIL+1)); echo "    -> FAIL ($2)"; fi; }
Z1=$(field slider '^Zoom$' | cut -f2)
printf "ctrl+pageup "; "$AD" press ctrl+pageup --window-id "$W" | r; echo; sleep 1
Z2=$(field slider '^Zoom$' | cut -f2); [ "$Z2" != "$Z1" ] && check yes "zoom $Z1 -> $Z2" || check no "zoom stayed $Z1"
printf "ctrl+e "; "$AD" press ctrl+e --window-id "$W" | r; echo; sleep 2
read REF V < <(field textfield '^Width:')
[ -n "$REF" ] && check yes "Image Properties open, width $V" || check no "no Image Properties"
NEW=$(( ${V:-300} + 40 ))
printf "type %s into Width " "$NEW"; "$AD" press ctrl+a --window-id "$W" >/dev/null 2>&1; "$AD" type "$REF" "$NEW" | r; echo
printf "return "; "$AD" press return --window-id "$W" | r; echo; sleep 2
"$AD" press ctrl+e --window-id "$W" >/dev/null; sleep 2
read REF V2 < <(field textfield '^Width:')
[ "$V2" = "$NEW" ] && check yes "image width now $V2" || check no "image width $V2, wanted $NEW"
printf "escape "; "$AD" press escape --window-id "$W" | r; echo; sleep 1
[ -z "$(field textfield '^Width:' | cut -f1)" ] && check yes "Image Properties closed" || check no "Image Properties still open"
for TOOL in Pencil Text; do
  printf "click %s " "$TOOL"; "$AD" click "$(field switch "^$TOOL\$" | cut -f1)" | r; echo; sleep 1
  STATUS=$(tree | awk -F'\t' '$3 ~ /^Using .* on Canvas$/ {print $3; exit}')
  [ "$STATUS" = "Using $TOOL tool on Canvas" ] && check yes "$STATUS" || check no "status: $STATUS"
done
echo "== $PASS passed, $FAIL failed"
[ "$WHERE" = person ] && { AFTER=$(FG); [ "$BEFORE" = "$AFTER" ] && echo "    foreground unchanged ($AFTER)" || echo "    FOREGROUND CHANGED: $BEFORE -> $AFTER"; }
if [ -n "$PID" ] && ! echo " $EARLIER " | grep -q " $PID "; then
  powershell -NoProfile -Command "Stop-Process -Id $PID -Force -ErrorAction SilentlyContinue"
else
  echo "    (Paint reused process $PID, which was already running: left open)"
fi
