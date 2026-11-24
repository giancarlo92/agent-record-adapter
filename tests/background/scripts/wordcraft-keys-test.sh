#!/usr/bin/env bash
# Part of Agent Record Adapter (see NOTICE).
# Keys and text in a deactivated window, in an app that drops keys unless it believes it is active
# and focused (WordCraft, a winit/egui app), on the hidden work desktop: type without activating it,
# Ctrl+A, Ctrl+B, Ctrl+S, then save from the dialog and read the file back with wordcraft-cli.
# The Save dialog's fields are found by their ids (1001 the file name, 1 the Save button), so the
# Windows language does not matter.
# With "person" as second argument it runs on the person's own desktop instead: WordCraft opens
# behind their windows (deactivated by them), and the window in front must stay the same.
# Usage: wordcraft-keys-test.sh <agent-record.exe> [person]
set -u
AD="$1"; WHERE="${2:-hidden}"
export AGENT_DESKTOP_HOME="$LOCALAPPDATA/DesktopRecorder/tmp/js-state"
[ "$WHERE" = person ] || export AGENT_DESKTOP_WORK_DESKTOP=artest
FG() { powershell -NoProfile -ExecutionPolicy Bypass -File "$(cd "$(dirname "$0")" && pwd)/foreground.ps1"; }
BEFORE=$(FG)
WC="C:\\Program Files\\WordCraft\\wordcraft.exe"; CLI="C:/Program Files/WordCraft/wordcraft-cli.exe"
OUT="$USERPROFILE/AprendeReplica-e2e/enter-test/wordcraft-keys.docx"; rm -f "$OUT"
# Only the WordCraft this test starts is closed at the end (by the pid launch reports).
OUTF="$LOCALAPPDATA/DesktopRecorder/tmp/wordcraft-launch.json"
"$AD" launch "$WC" --timeout 30000 >"$OUTF"
PID=$(node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{try{console.log(JSON.parse(s).data.pid)}catch(e){console.log('')}})" <"$OUTF")
win() { "$AD" list-windows --app wordcraft.exe | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const w=o.data.find(w=>new RegExp(process.argv[1]).test(w.title));console.log(w?w.id:'')})" "$1"; }
refs() { "$AD" snapshot --window-id "$1" | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const out=[];(function w(n){if(!n)return;if(n.ref_id)out.push(n.ref_id+'\t'+(n.role||'')+'\t'+(n.name||'')+'\t'+JSON.stringify(n.native_id||''));(n.children||[]).forEach(w)})(o.data&&o.data.tree);console.log(out.join('\n'))})"; }
r() { node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);console.log('   ',o.ok?'ok '+(((o.data||{}).steps||[]).map(x=>x.label).join(',')):'ERR '+o.error.code+' '+o.error.message)})"; }
W=""; for i in $(seq 1 20); do W=$(win 'WordCraft'); [ -n "$W" ] && break; sleep 1; done
NEW=$(refs "$W" | awk -F'\t' '$2=="button" && $3=="Blank document" {print $1; exit}')
"$AD" click "$NEW" >/dev/null; sleep 2
W=$(win 'Document1')
# A window takes the activation on that desktop and goes away, as the person's windows take it
# on theirs: WordCraft is left deactivated (it drops keys unless told it is active and focused).
"$AD" launch "C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe" --arg=-NoProfile --arg=-Command --arg="Start-Sleep -Seconds 2" --timeout 30000 >/dev/null 2>&1
sleep 4
FRONT=$("$AD" list-windows --app wordcraft.exe | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const w=o.data.find(w=>w.id===process.argv[1]);console.log(w&&w.is_focused?'active':'deactivated')})" "$W")
echo "window $W ($FRONT)"
echo "== type, the window behind (never activated)"; "$AD" type "$W" "Hola mundo" | r
echo "== ctrl+a"; "$AD" press ctrl+a --window-id "$W" | r
echo "== ctrl+b"; "$AD" press ctrl+b --window-id "$W" | r
echo "== ctrl+s"; "$AD" press ctrl+s --window-id "$W" | r
D=""; for i in $(seq 1 10); do D=$("$AD" list-windows --app wordcraft.exe | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const w=o.data.find(w=>w.title&&!/WordCraft/.test(w.title));console.log(w?w.id:'')})"); [ -n "$D" ] && break; sleep 1; done
LIST=$(refs "$D")
NAME=$(echo "$LIST" | awk -F'\t' '$4 ~ /"1001"/ && ($2=="textfield"||$2=="combobox") {print $1; exit}')
SAVE=$(echo "$LIST" | awk -F'\t' '$4 ~ /"1"/ && $2=="button" {print $1; exit}')
"$AD" set-value "$NAME" "$(cygpath -w "$OUT")" >/dev/null; "$AD" click "$SAVE" >/dev/null
for i in $(seq 1 10); do [ -f "$OUT" ] && break; sleep 1; done
"$CLI" inspect "$(cygpath -w "$OUT")" 2>/dev/null | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{try{const o=JSON.parse(s);const b=o.blocks[0];const bold=b.runs.every(r=>r.props&&r.props.bold);console.log('    file text: '+JSON.stringify(b.text)+', bold: '+bold);console.log(b.text==='Hola mundo'&&bold?'    -> PASS':'    -> FAIL')}catch(e){console.log('    -> FAIL (no file)')}})"
[ "$WHERE" = person ] && { AFTER=$(FG); [ "$BEFORE" = "$AFTER" ] && echo "    foreground unchanged ($AFTER)" || echo "    FOREGROUND CHANGED: $BEFORE -> $AFTER"; }
[ -n "$PID" ] && powershell -NoProfile -Command "Stop-Process -Id $PID -Force -ErrorAction SilentlyContinue"
