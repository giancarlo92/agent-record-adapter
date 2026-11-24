#!/usr/bin/env bash
# Part of Agent Record Adapter (see NOTICE). Ported from the desktop-recorder-mcp fork.
# Opening a "Select files" button's file picker (a hidden input opened from JavaScript, like YouTube
# Studio) in a background Chrome. Usage: studio-picker-test.sh <agent-record.exe> <plain|cdp> [keep]
set -u
AD="$1"; MODE="$2"; KEEP="${3:-}"
export AGENT_DESKTOP_HOME="$LOCALAPPDATA/AgentRecordAdapter/tmp/js-state" TEMP="$LOCALAPPDATA/AgentRecordAdapter/tmp" TMP="$LOCALAPPDATA/AgentRecordAdapter/tmp"
unset AGENT_DESKTOP_SESSION
D="$USERPROFILE/AprendeReplica-e2e/enter-test"; WD=$(cygpath -w "$D"); F="$D/studio-snap.json"
FORM="$D/studio.html"
PROFILE="profile-studio-$MODE"
CDP=""; [ "$MODE" = cdp ] && CDP="--cdp"
ref() { "$AD" snapshot --window-id $W -i > "$F"; node -e "const o=require(process.argv[1]);let h='';(function w(n){if(!n||h)return;if(n.role===process.argv[2]&&new RegExp(process.argv[3]).test(n.name||'')&&n.ref_id)h=n.ref_id;(n.children||[]).forEach(w)})(o.data&&o.data.tree);console.log(h)" "$(cygpath -w "$F")" "$1" "$2"; }
fg() { powershell -NoProfile -ExecutionPolicy Bypass -File "$(cygpath -w "$(dirname "$0")/foreground.ps1")" | tr -d '\r'; }
"$AD" launch "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe" --no-attach $CDP "--arg=--user-data-dir=$WD\\$PROFILE" --arg=--no-first-run "--arg=file:///$(cygpath -m "$FORM")" --timeout 30000 >/dev/null
sleep 4
W=$("$AD" list-windows --app chrome.exe | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const w=o.data.find(w=>/^Northwind Studio/.test(w.title));console.log(w?w.id:'')})")
echo "[$MODE] window $W"
"$AD" restore --window-id $W >/dev/null; sleep 2
FG0=$(fg)
for i in $(seq 1 10); do C=$(ref menubutton '^Create$'); [ -n "$C" ] && break; sleep 1; done
"$AD" click "$C" >/dev/null; sleep 1
# Some upload pages open a menu first ("Upload videos"); this one opens the dialog directly.
U=$(ref menuitem 'Upload videos'); [ -n "$U" ] && { "$AD" click "$U" >/dev/null; sleep 1; }
S=$(ref button '^Select files$')
echo "  Select files: $S"
"$AD" click "$S" | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);console.log('  click:',o.ok?(o.data.disposition.delivery+' ['+o.data.steps.map(x=>x.label).join(', ')+']'):('ERR '+o.error.code+' '+o.error.message))})"
"$AD" wait --event window-opened --window Open --timeout 6000 | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);console.log('  picker:',o.ok?'opened '+o.data.event.window_id:'not opened ('+o.error.code+')')})"
FG1=$(fg)
# A change to a window of the person's own (they switched windows) is not the test's; a change to
# chrome is.
[ "$FG0" = "$FG1" ] && echo "  foreground unchanged ($FG1)" || echo "  foreground: $FG0 -> $FG1"
[ -n "$KEEP" ] && { echo "W=$W" > "$D/studio-ids.txt"; exit 0; }
DLG=$("$AD" list-windows | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const w=o.data.find(w=>/^(Open|Abrir)$/.test(w.title));console.log(w?w.id:'')})")
if [ -n "$DLG" ]; then "$AD" snapshot --window-id $DLG -i --timeout-ms 20000 > "$F"; X=$(node -e "const o=require(process.argv[1]);let h='';(function w(n){if(!n||h)return;if(n.role==='button'&&/^(Cancel|Cancelar)$/.test(n.name||''))h=n.ref_id;(n.children||[]).forEach(w)})(o.data.tree);console.log(h)" "$(cygpath -w "$F")"); "$AD" click "$X" >/dev/null; fi
powershell -NoProfile -Command "Get-CimInstance Win32_Process -Filter \"Name='chrome.exe'\" | Where-Object { \$_.CommandLine -like '*$PROFILE*' -and \$_.CommandLine -notlike '*--type=*' } | ForEach-Object { Stop-Process -Id \$_.ProcessId -ErrorAction SilentlyContinue }"
