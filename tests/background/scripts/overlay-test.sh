#!/usr/bin/env bash
# Part of Agent Record Adapter (see NOTICE). Ported from the desktop-recorder-mcp fork.
# The drawn cursor in a CLI session: hidden before, at the element after a background click on the
# SGH-like page, gone after session end. Usage: overlay-test.sh <agent-record.exe>
set -u
AD="$1"; S="$(dirname "$0")"
export AGENT_DESKTOP_HOME="$LOCALAPPDATA/DesktopRecorder/tmp/js-state" TEMP="$LOCALAPPDATA/DesktopRecorder/tmp" TMP="$LOCALAPPDATA/DesktopRecorder/tmp"
D="$USERPROFILE/AprendeReplica-e2e/enter-test"; WD=$(cygpath -w "$D"); F="$D/ov-snap.json"
where() { powershell -NoProfile -ExecutionPolicy Bypass -File "$(cygpath -w "$S/overlay-where.ps1")"; }
"$AD" launch "C:\Program Files\Google\Chrome\Application\chrome.exe" --no-attach "--arg=--user-data-dir=$WD\profile-ov" --arg=--no-first-run "--arg=file:///$(cygpath -m "$D/sgh-like.html")" --timeout 30000 >/dev/null
sleep 3
W=$("$AD" list-windows --app chrome.exe | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const w=o.data.find(w=>/SGH-like/.test(w.title));console.log(w?w.id:'')})"); echo "window $W"
"$AD" restore --window-id $W >/dev/null; sleep 2
SID=$("$AD" session start --name overlay-test --cursor | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);console.log(o.ok?o.data.session_id:'ERR '+JSON.stringify(o.error))})"); echo "session $SID"
export AGENT_DESKTOP_SESSION="$SID"
"$AD" cursor-overlay enable --label "Overlay test" | head -c 160; echo
echo "before: $(where)"
for i in $(seq 1 10); do "$AD" snapshot --window-id $W -i --include-bounds > "$F"; R=$(node -e "const o=require(process.argv[1]);let h='';(function w(n){if(!n||h)return;if(n.role==='option'&&/^Personal/.test(n.name||'')&&n.ref_id)h=n.ref_id+' '+JSON.stringify(n.bounds||{});(n.children||[]).forEach(w)})(o.data&&o.data.tree);console.log(h)" "$(cygpath -w "$F")"); [ -n "$R" ] && break; sleep 1; done
echo "item \"Personal\" at: $R"
"$AD" click "${R%% *}" | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);console.log('click:',o.ok?o.data.disposition.delivery:'ERR '+o.error.code)})"
echo "after click: $(where)"
sleep 1; echo "1 s later: $(where)"
"$AD" cursor-overlay disable >/dev/null; "$AD" session end >/dev/null; unset AGENT_DESKTOP_SESSION
sleep 1; echo "after end: $(where)"
powershell -NoProfile -Command "Get-CimInstance Win32_Process -Filter \"Name='chrome.exe'\" | Where-Object { \$_.CommandLine -like '*profile-ov*' -and \$_.CommandLine -notlike '*--type=*' } | ForEach-Object { Stop-Process -Id \$_.ProcessId -ErrorAction SilentlyContinue }"
