#!/usr/bin/env bash
# Part of Agent Record Adapter (see NOTICE).
# secret fill on a password field with no name, label or id (login-unnamed.html):
# upstream refuses its ref as not unique; agent-record finds it by its place.
# A throwaway secret is made from random bytes, never printed, and deleted at the end.
# Usage: secret-unnamed-test.sh <agent-record.exe>
set -u
AD="$1"
export AGENT_DESKTOP_HOME="$LOCALAPPDATA/AgentRecordAdapter/tmp/js-state" TEMP="$LOCALAPPDATA/AgentRecordAdapter/tmp" TMP="$LOCALAPPDATA/AgentRecordAdapter/tmp"
D="$USERPROFILE/AprendeReplica-e2e/enter-test"; WD=$(cygpath -w "$D"); F="$D/login.json"
NAME=ara-test-unnamed
node -e "process.stdout.write(require('crypto').randomBytes(6).toString('hex')+'\n')" | "$AD" secret set $NAME --app chrome.exe --stdin >/dev/null
"$AD" launch "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe" --no-attach "--arg=--user-data-dir=$WD\\profile-login" --arg=--no-first-run "--arg=file:///$(cygpath -m "$D/login-unnamed.html")" --timeout 30000 >/dev/null
sleep 3
W=$("$AD" list-windows --app chrome.exe | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const w=o.data.find(w=>/Unnamed login/.test(w.title));console.log(w?w.id:'')})")
echo "window $W"
PW=""
for i in $(seq 1 15); do
  "$AD" snapshot --window-id $W > "$F"
  PW=$(node -e "const o=require(process.argv[1]);let h='';(function w(n){if(!n||h)return;if(n.role==='textfield'&&(n.states||[]).includes('secure')&&n.ref_id)h=n.ref_id;(n.children||[]).forEach(w)})(o.data&&o.data.tree);console.log(h)" "$(cygpath -w "$F")")
  [ -n "$PW" ] && break; sleep 1
done
echo "password field $PW"
echo "== upstream alone (background off): set-value on that ref"
AGENT_DESKTOP_BACKGROUND=0 "$AD" set-value "$PW" x | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);console.log('   ',o.ok?'ok':o.error.code+' '+((o.error.details||{}).kind||''))})"
echo "== agent-record secret fill"
"$AD" secret fill "$PW" $NAME | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);console.log('   ',o.ok?('ok, resolved by '+(o.data.resolved_by||'ref')+(o.data.refusal_replaced?' (replaced: '+o.data.refusal_replaced+')':'')):('ERR '+o.error.code+' '+o.error.message))})"
"$AD" wait --text "pw-len: 12" --window-id $W --timeout 5000 | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);console.log(o.ok?'    -> PASS (the page has 12 characters)':'    -> FAIL ('+o.error.message+')')})"
"$AD" secret delete $NAME >/dev/null
powershell -NoProfile -Command "Get-CimInstance Win32_Process -Filter \"Name='chrome.exe'\" | Where-Object { \$_.CommandLine -like '*profile-login*' -and \$_.CommandLine -notlike '*--type=*' } | ForEach-Object { Stop-Process -Id \$_.ProcessId -ErrorAction SilentlyContinue }"
