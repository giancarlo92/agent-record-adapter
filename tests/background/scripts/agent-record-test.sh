#!/usr/bin/env bash
# Part of Agent Record Adapter (see NOTICE). Ported from the desktop-recorder-mcp fork.
# Agent actions in a background recording: record start on the SGH-like window, the agent clicks,
# fills, selects a radio; record stop; the recording must list those steps with by:"agent".
# Usage: agent-record-test.sh <agent-record.exe>
set -u
AD="$1"
export AGENT_DESKTOP_HOME="$LOCALAPPDATA/AgentRecordAdapter/tmp/js-state" TEMP="$LOCALAPPDATA/AgentRecordAdapter/tmp" TMP="$LOCALAPPDATA/AgentRecordAdapter/tmp"
D="$USERPROFILE/AprendeReplica-e2e/enter-test"; WD=$(cygpath -w "$D"); F="$D/rec-snap.json"; OUT="$D/agent-record.jsonl"; rm -f "$OUT"
"$AD" launch "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe" --no-attach "--arg=--user-data-dir=$WD\\profile-rec" --arg=--no-first-run "--arg=file:///$(cygpath -m "$D/sgh-like.html")" --timeout 30000 >/dev/null
sleep 3
W=$("$AD" list-windows --app chrome.exe | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const w=o.data.find(w=>/SGH-like/.test(w.title));console.log(w?w.id:'')})")
echo "window $W"
q() { node -e "$1" "$(cygpath -w "$F")"; }
for i in $(seq 1 15); do "$AD" snapshot --window-id $W > "$F"; n=$(q "const o=require(process.argv[1]);let c=0;(function w(n){if(!n)return;if(n.role==='radiobutton')c++;(n.children||[]).forEach(w)})(o.data.tree);console.log(c)"); [ "$n" -gt 0 ] && break; sleep 1; done
"$AD" record start --window-id $W --out "$(cygpath -w "$OUT")" --no-indicator | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);console.log('record start:',o.ok?o.data.id:JSON.stringify(o.error))})"
"$AD" snapshot --window-id $W > "$F"
OPT=$(q "const o=require(process.argv[1]);let h='';(function w(n){if(!n||h)return;if(n.role==='option'&&/^Personal/.test(n.name||'')&&n.ref_id)h=n.ref_id;(n.children||[]).forEach(w)})(o.data.tree);console.log(h)")
"$AD" click "$OPT" >/dev/null && echo "agent: click Personal"
"$AD" snapshot --window-id $W > "$F"
FIELD=$(q "const o=require(process.argv[1]);const r=[];(function w(n){if(!n)return;if(n.role==='textfield'&&!n.name&&n.ref_id)r.push(n.ref_id);(n.children||[]).forEach(w)})(o.data.tree);console.log(r[1]||'')")
"$AD" set-value "$FIELD" "Quispe" >/dev/null && echo "agent: set-value Apellido Materno"
"$AD" snapshot --window-id $W > "$F"
R=$(q "const o=require(process.argv[1]);let h='';(function w(n){if(!n||h)return;if(n.role==='radiobutton'&&n.name==='Pago directo'&&n.ref_id)h=n.ref_id;(n.children||[]).forEach(w)})(o.data.tree);console.log(h)")
"$AD" click "$R" >/dev/null && echo "agent: click Pago directo"
"$AD" press return --window-id $W >/dev/null && echo "agent: press return (window, not an element action)"
sleep 1
"$AD" record stop | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);console.log('record stop:',o.ok?('actions='+o.data.actions):JSON.stringify(o.error))})"
echo "--- recorded events"
node -e "require('fs').readFileSync(process.argv[1],'utf8').trim().split('\n').map(l=>JSON.parse(l)).forEach(e=>{const el=e.element||{};console.log(' ',e.kind, e.t_ms??'', e.by||'', el.role||'', JSON.stringify(el.name||el.label||''), e.value!==undefined?('value='+JSON.stringify(e.value)):'', e.window?('win='+e.window.id):'', e.match_count?('match='+e.match_count):'')})" "$(cygpath -w "$OUT")"
powershell -NoProfile -Command "Get-CimInstance Win32_Process -Filter \"Name='chrome.exe'\" | Where-Object { \$_.CommandLine -like '*profile-rec*' -and \$_.CommandLine -notlike '*--type=*' } | ForEach-Object { Stop-Process -Id \$_.ProcessId -ErrorAction SilentlyContinue }"
