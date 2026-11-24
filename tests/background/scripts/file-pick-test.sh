#!/usr/bin/env bash
# Part of Agent Record Adapter (see NOTICE). Ported from the desktop-recorder-mcp fork.
# The Chrome file picker in a background recording of the Chrome window (SGH report, Desktop Recorder row).
# Usage: file-pick-test.sh <agent-record.exe> [keep]
set -u
AD="$1"; KEEP="${2:-}"
export AGENT_DESKTOP_HOME="$LOCALAPPDATA/AgentRecordAdapter/tmp/js-state" TEMP="$LOCALAPPDATA/AgentRecordAdapter/tmp" TMP="$LOCALAPPDATA/AgentRecordAdapter/tmp"
D="$USERPROFILE/AprendeReplica-e2e/enter-test"; WD=$(cygpath -w "$D"); F="$D/fp-snap.json"; OUT="$D/file-pick.jsonl"; rm -f "$OUT"
q() { node -e "$1" "$(cygpath -w "$F")" "${@:2}"; }
findref() { q "const o=require(process.argv[1]);const re=new RegExp(process.argv[3]);let h='';(function w(n){if(!n||h)return;if(n.role===process.argv[2]&&re.test(n.name||'')&&n.ref_id)h=n.ref_id;(n.children||[]).forEach(w)})(o.data&&o.data.tree);console.log(h)" "$1" "$2"; }
wins() { "$AD" list-windows | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);o.data.filter(w=>/chrome/i.test(w.app)).forEach(w=>console.log('  ',w.id,w.pid,JSON.stringify(w.title)))})"; }
"$AD" launch "C:\Program Files\Google\Chrome\Application\chrome.exe" --no-attach "--arg=--user-data-dir=$WD\profile-fp" --arg=--no-first-run "--arg=file:///$(cygpath -m "$D/file-pick.html")" --timeout 30000 >/dev/null
sleep 3
W=$("$AD" list-windows --app chrome.exe | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const w=o.data.find(w=>/File pick test/.test(w.title));console.log(w?w.id:'')})")
echo "window $W"
"$AD" restore --window-id $W >/dev/null; sleep 2
"$AD" record start --window-id $W --out "$(cygpath -w "$OUT")" --no-indicator | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);console.log('record start:',o.ok?o.data.id:JSON.stringify(o.error))})"
for i in $(seq 1 20); do "$AD" snapshot --window-id $W -i > "$F"; B=$(findref button "Archivo|Choose|Elegir|Seleccionar"); [ -n "$B" ] && break; [ $i = 5 ] && head -c 300 "$F"; sleep 1; done
echo "file button: $B"
"$AD" click "$B" | head -c 300; echo
sleep 2
echo "windows after the click:"; wins
DLG=$("$AD" list-windows | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const w=o.data.find(w=>/^(Abrir|Open)$/.test(w.title));console.log(w?w.id:'')})")
echo "dialog $DLG"
powershell -NoProfile -Command "Add-Type -Namespace W -Name U -MemberDefinition '[DllImport(\"user32.dll\")] public static extern System.IntPtr GetAncestor(System.IntPtr h,uint f); [DllImport(\"user32.dll\")] public static extern System.IntPtr GetWindow(System.IntPtr h,uint c); [DllImport(\"user32.dll\")] public static extern uint GetWindowThreadProcessId(System.IntPtr h,out uint p);'; \$h=[IntPtr]${DLG#w-}; \$o=[W.U]::GetWindow(\$h,4); \$r=[W.U]::GetAncestor(\$h,3); \$p=0; [void][W.U]::GetWindowThreadProcessId(\$o,[ref]\$p); \$q=0; [void][W.U]::GetWindowThreadProcessId(\$h,[ref]\$q); 'owner w-'+\$o+' (pid '+\$p+')  rootowner w-'+\$r+'  dialog pid '+\$q"
F="$D/fp-dlg.json"
"$AD" snapshot --window-id $DLG -i > "$F"
NAME=$(findref textfield '^(Nombre|File name)')
# The dialog's own Open: a button or a split menubutton, not the arrow "Open" inside a list.
OPEN=$(q "const o=require(process.argv[1]);const r=[];(function w(n,inList){if(!n)return;if(!inList&&/^(button|menubutton)$/.test(n.role)&&/^(Abrir|Open)$/.test(n.name||'')&&n.ref_id)r.push(n.ref_id);(n.children||[]).forEach(c=>w(c,inList||n.role==='combobox'))})(o.data.tree,false);console.log(r[r.length-1]||'')")
echo "name field $NAME, open $OPEN"
"$AD" set-value "$NAME" "$(cygpath -w "$D/files/sample.csv")" | head -c 300; echo
sleep 1
if [ -z "$KEEP" ]; then "$AD" click "$OPEN" > "$D/fp-open.json" 2>&1; echo "open click exit=$?"; head -c 300 "$D/fp-open.json"; echo; fi
sleep 2
F="$D/fp-snap.json"; "$AD" snapshot --window-id $W > "$F"; q "const o=require(process.argv[1]);let t='';(function w(n){if(!n)return;if(/picked|none/.test(n.name||''))t+=n.name+' ';(n.children||[]).forEach(w)})(o.data.tree);console.log('page says:',t)"
"$AD" record stop | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);console.log('record stop:',o.ok?('actions='+o.data.actions):JSON.stringify(o.error))})"
echo "--- recorded events"
node -e "const fs=require('fs');if(!fs.existsSync(process.argv[1])){console.log('no file');process.exit()}fs.readFileSync(process.argv[1],'utf8').trim().split('\n').filter(Boolean).map(l=>JSON.parse(l)).forEach(e=>{const el=e.element||{};console.log(' ',e.kind, e.by||'', el.role||'', JSON.stringify(el.name||el.label||''), e.value!==undefined?('value='+JSON.stringify(e.value)):'', e.typed!==undefined?('typed='+JSON.stringify(e.typed)):'', e.window?('win='+e.window.id+' '+JSON.stringify(e.window.title)+(e.window.url?' url='+e.window.url:'')):'', e.window_owner?('owner='+e.window_owner):'')})" "$(cygpath -w "$OUT")"
[ -n "$KEEP" ] && exit 0
powershell -NoProfile -Command "Get-CimInstance Win32_Process -Filter \"Name='chrome.exe'\" | Where-Object { \$_.CommandLine -like '*profile-fp*' -and \$_.CommandLine -notlike '*--type=*' } | ForEach-Object { Stop-Process -Id \$_.ProcessId -ErrorAction SilentlyContinue }"
