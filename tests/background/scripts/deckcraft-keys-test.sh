#!/usr/bin/env bash
# Part of Agent Record Adapter (see NOTICE).
# Keys and text in a deactivated DeckCraft (a winit app), on the hidden work desktop: a blank
# presentation, a window takes the activation and goes away, then: type a note into the notes
# field (a field DeckCraft draws, that offers only the focus), click "New Slide" (the status reads
# "Slide 2 of 2"), Ctrl+S (its "Save (Ctrl+S, Alt+2)" button), save from the dialog by its ids, and
# read the file back with deckcraft-cli. DeckCraft shows no key for a new slide.
# Only the DeckCraft this test starts is closed at the end (by the pid launch reports).
# With "person" as second argument it runs on the person's own desktop instead: DeckCraft opens
# behind their windows, and the window in front must stay the same.
# Usage: deckcraft-keys-test.sh <agent-record.exe> [person]
set -u
AD="$1"; WHERE="${2:-hidden}"
export AGENT_DESKTOP_HOME="$LOCALAPPDATA/DesktopRecorder/tmp/js-state"
[ "$WHERE" = person ] || export AGENT_DESKTOP_WORK_DESKTOP=artest
HERE="$(cd "$(dirname "$0")" && pwd)"
FG() { powershell -NoProfile -ExecutionPolicy Bypass -File "$HERE/foreground.ps1"; }
BEFORE=$(FG)
DC="C:\\Program Files\\DeckCraft\\deckcraft.exe"; CLI="C:/Program Files/DeckCraft/deckcraft-cli.exe"
PS="C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"
OUT="$USERPROFILE/AprendeReplica-e2e/enter-test/deckcraft-keys.pptx"; rm -f "$OUT"
OUTF="$LOCALAPPDATA/DesktopRecorder/tmp/deckcraft-launch.json"
"$AD" launch "$DC" --timeout 30000 >"$OUTF"
PID=$(node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{try{console.log(JSON.parse(s).data.pid)}catch(e){console.log('')}})" <"$OUTF")
win() { "$AD" list-windows --app deckcraft.exe | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const w=o.data.find(w=>w.pid==process.argv[1]&&new RegExp(process.argv[2]).test(w.title));console.log(w?w.id+' '+(w.is_focused?'active':'deactivated'):'')})" "$PID" "$1"; }
tree() { "$AD" snapshot --window-id "$1" | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const out=[];(function w(n){if(!n)return;out.push((n.ref_id||'-')+'\t'+(n.role||'')+'\t'+(n.name||'')+'\t'+JSON.stringify(n.native_id||'')+'\t'+(n.value||''));(n.children||[]).forEach(w)})(o.data&&o.data.tree);console.log(out.join('\n'))})"; }
r() { node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);console.log('   ',o.ok?'ok '+(((o.data||{}).steps||[]).map(x=>x.label).join(',')):'ERR '+o.error.code+' '+o.error.message)})"; }
W=""; for i in $(seq 1 20); do read W ST < <(win 'DeckCraft'); [ -n "$W" ] && break; sleep 1; done
BLANK=$(tree "$W" | awk -F'\t' '$2=="button" && $3=="Blank Presentation" {print $1; exit}')
"$AD" click "$BLANK" >/dev/null; sleep 3
[ "$WHERE" = person ] || { "$AD" launch "$PS" --arg=-NoProfile --arg=-Command --arg="Start-Sleep -Seconds 2" --timeout 30000 >/dev/null 2>&1; sleep 4; }
read W ST < <(win 'DeckCraft')
echo "window $W ($ST)"
status() { tree "$W" | awk -F'\t' '$2=="statictext" && $3 ~ /^Slide [0-9]+ of [0-9]+$/ {print $3; exit}'; }
echo "    status: $(status)"
NOTES=$(tree "$W" | awk -F'\t' '$2=="textfield" && $3=="Click to add notes" {print $1; exit}')
echo "== type into the notes"; "$AD" type "$NOTES" "Nota de prueba" | r
# The notes field is named "Click to add notes" only while it is empty: read by its role.
echo "    notes: $(tree "$W" | awk -F'\t' '$2=="textfield" {print $5; exit}')"
NEWSLIDE=$(tree "$W" | awk -F'	' '$2=="button" && $3=="New Slide" {print $1; exit}')
echo "== New Slide"; "$AD" click "$NEWSLIDE" | r; sleep 1
echo "    status: $(status)"
echo "== ctrl+s"; "$AD" press ctrl+s --window-id "$W" | r
D=""; for i in $(seq 1 10); do D=$("$AD" list-windows --app deckcraft.exe | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const w=o.data.find(w=>w.pid==process.argv[1]&&w.title&&!/DeckCraft/.test(w.title));console.log(w?w.id:'')})" "$PID"); [ -n "$D" ] && break; sleep 1; done
LIST=$(tree "$D")
NAME=$(echo "$LIST" | awk -F'\t' '$4 ~ /"1001"/ && ($2=="textfield"||$2=="combobox") {print $1; exit}')
SAVE=$(echo "$LIST" | awk -F'\t' '$4 ~ /"1"/ && $2=="button" {print $1; exit}')
"$AD" set-value "$NAME" "$(cygpath -w "$OUT")" >/dev/null; "$AD" click "$SAVE" >/dev/null
for i in $(seq 1 10); do [ -f "$OUT" ] && break; sleep 1; done
"$CLI" info "$(cygpath -w "$OUT")" 2>/dev/null | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{try{const o=JSON.parse(s);const slides=o.slides||[];const notes=slides.some(x=>x.notes==='Nota de prueba');console.log('    file: '+slides.length+' slides, notes '+(notes?'saved':'missing'));console.log(slides.length===2&&notes?'    -> PASS':'    -> FAIL')}catch(e){console.log('    -> FAIL (no file: '+e.message+')')}})"
[ "$WHERE" = person ] && { AFTER=$(FG); [ "$BEFORE" = "$AFTER" ] && echo "    foreground unchanged ($AFTER)" || echo "    FOREGROUND CHANGED: $BEFORE -> $AFTER"; }
[ -n "$PID" ] && powershell -NoProfile -Command "Stop-Process -Id $PID -Force -ErrorAction SilentlyContinue"
