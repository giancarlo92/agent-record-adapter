#!/usr/bin/env bash
# Part of Agent Record Adapter (see NOTICE). Ported from the desktop-recorder-mcp fork.
# DR-06: navigate keeps the asked address.
#  cdp:    a single-page app that sends a full load home; navigate must reach the asked route (in-app retry).
#  native: the address bar's inline completion of an earlier, longer address must not replace the asked one.
# Usage: nav-test.sh <agent-record.exe> <cdp|native>
set -u
AD="$1"; KIND="$2"
export AGENT_DESKTOP_HOME="$LOCALAPPDATA/AgentRecordAdapter/tmp/js-state" TEMP="$LOCALAPPDATA/AgentRecordAdapter/tmp" TMP="$LOCALAPPDATA/AgentRecordAdapter/tmp"
D="$USERPROFILE/AprendeReplica-e2e/enter-test"; WD=$(cygpath -w "$D"); BASE="file:///$(cygpath -m "$D")"
CDP=""; [ "$KIND" = cdp ] && CDP="--cdp"
"$AD" launch "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe" $CDP --no-attach "--arg=--user-data-dir=$WD\\profile-nav-$KIND" --arg=--no-first-run "--arg=$BASE/spa.html" --timeout 30000 >/dev/null
sleep 3
W=$("$AD" list-windows --app chrome.exe | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const w=o.data.find(w=>/SPA-like|sgh-like|SGH-like/.test(w.title));console.log(w?w.id:'')})")
echo "##### $KIND window $W"
view() { "$AD" snapshot --window-id $W | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);let v='';(function w(n){if(!n||v)return;if(/^vista:/.test(n.name||''))v=n.name;(n.children||[]).forEach(w)})(o.data&&o.data.tree);console.log(v)})"; }
nav() { "$AD" navigate --window-id $W "$1" | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);console.log('   ',o.ok?('reached='+o.data.reached+' url='+decodeURIComponent(o.data.url||'').replace(/^.*enter-test\//,'')):('ERR '+o.error.code+' '+o.error.message))})"; }
if [ "$KIND" = cdp ]; then
  echo "== full load of ?r=usuarios (the app sends it home; navigate retries in-app)"; nav "$BASE/spa.html?r=usuarios"
  v=$(view); [ "$v" = "vista: usuarios" ] && echo "    -> PASS ($v)" || echo "    -> FAIL ($v)"
  echo "== --in-app ?r=home"; "$AD" navigate --window-id $W --in-app "$BASE/spa.html?r=home" >/dev/null
  v=$(view); [ "$v" = "vista: home" ] && echo "    -> PASS ($v)" || echo "    -> FAIL ($v)"
else
  echo "== seed history with a longer address"; nav "$BASE/sgh-like.html?autocompletar=si"
  echo "== navigate to the shorter address (inline completion must not win)"; nav "$BASE/sgh-like.html"
  U=$("$AD" list-windows --app chrome.exe | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const w=o.data.find(w=>w.id==='$W');console.log(w?w.title:'')})")
  echo "    window title now: $U"
  echo "== navigate back to spa.html"; nav "$BASE/spa.html"
  v=$(view); [ "$v" = "vista: home" ] && echo "    -> PASS ($v)" || echo "    -> FAIL ($v)"
fi
powershell -NoProfile -Command "Get-CimInstance Win32_Process -Filter \"Name='chrome.exe'\" | Where-Object { \$_.CommandLine -like '*profile-nav-$KIND*' -and \$_.CommandLine -notlike '*--type=*' } | ForEach-Object { Stop-Process -Id \$_.ProcessId -ErrorAction SilentlyContinue }"
