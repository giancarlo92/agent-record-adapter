#!/usr/bin/env bash
# Part of Agent Record Adapter (see NOTICE).
# An agent's web actions through agent-browser (agent-record browser ...) land
# in a background recording, through the DevTools proxy: fill, radio click,
# select. Headless dedicated Chrome on its own port, so nothing shows.
# Usage: browser-record-test.sh <agent-record.exe> <agent-browser.exe> <signup-v1.html path>
set -u
AD="$1"; export AGENT_RECORD_BROWSER_BIN="$(cygpath -w "$2")"; FORM="$3"
export AGENT_DESKTOP_HOME="$LOCALAPPDATA/AgentRecordAdapter/tmp/js-state" AGENT_RECORD_CHROME_PORT=9433 AGENT_RECORD_CHROME_HEADLESS=1
D="$USERPROFILE/AprendeReplica-e2e/enter-test"; OUT="$D/browser-agent.jsonl"; rm -f "$OUT"
"$AD" browser open "file:///$(cygpath -m "$FORM")" >/dev/null 2>&1
"$AD" browser snapshot -i >/dev/null 2>&1
"$AD" record start --out "$(cygpath -w "$OUT")" --no-indicator >/dev/null
sleep 1
"$AD" browser fill @e3 "Rosa Quispe" >/dev/null 2>&1 && echo "agent: fill Full name"
"$AD" browser click @e12 >/dev/null 2>&1 && echo "agent: click Pro"
"$AD" browser select @e6 Peru >/dev/null 2>&1 && echo "agent: select Peru"
sleep 1
"$AD" record stop >/dev/null
echo "--- recorded events"
node -e "require('fs').readFileSync(process.argv[1],'utf8').trim().split('\n').map(l=>JSON.parse(l)).filter(e=>!/^recording_/.test(e.kind)).forEach(e=>{const el=e.element||{};console.log(' ',e.kind,e.by||'',e.driver||'',el.role||el.tag||'',JSON.stringify(el.label||el.name||el.id||''),e.value!==undefined?'value='+JSON.stringify(e.value):'',e.keys!==undefined?'keys='+JSON.stringify(e.keys):'')})" "$(cygpath -w "$OUT")"
"$AD" browser close >/dev/null 2>&1
