#!/usr/bin/env bash
# Part of Agent Record Adapter (see NOTICE).
# secret fill with a secret bound to a site: the field's page address is read in a way that does
# not depend on the browser's language (the page's own address, else the address bar by its
# class). The page is served on 127.0.0.1; the browser may run in any UI language (LANG_ARG,
# e.g. --lang=es). A secret bound to another site must still be refused.
# Throwaway secrets are made from random bytes, never printed, and deleted at the end.
# Any Chromium browser: BROWSER_EXE=<its program> (Chrome by default).
# The browser is closed as a person closes it (no "quit unexpectedly" prompt next time).
# Usage: secret-site-test.sh <agent-record.exe> [--lang=xx]
set -u
AD="$1"; LANG_ARG="${2:-}"
export AGENT_DESKTOP_HOME="$LOCALAPPDATA/DesktopRecorder/tmp/js-state" TEMP="$LOCALAPPDATA/DesktopRecorder/tmp" TMP="$LOCALAPPDATA/DesktopRecorder/tmp"
BROWSER_EXE="${BROWSER_EXE:-C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe}"; APP=$(basename "${BROWSER_EXE//\\//}"); BR=${APP%.exe}
# A new Edge profile signs into the Windows Microsoft account and syncs its extensions: not in a test profile.
EXTRA=""; [ "$BR" = msedge ] && EXTRA="--arg=--disable-features=msImplicitSignin"
LANGX=""; [ -n "$LANG_ARG" ] && LANGX="--arg=$LANG_ARG"
D="$USERPROFILE/AprendeReplica-e2e/enter-test"; WD=$(cygpath -w "$D"); F="$D/login-site.json"
PORT=8719
R="$(cd "$(dirname "$0")/../pages" && pwd)"
python -m http.server $PORT --bind 127.0.0.1 --directory "$(cygpath -w "$R")" >/dev/null 2>&1 &
SERVER=$!
sleep 1
GOOD=ara-test-site-good; BAD=ara-test-site-bad
for n in $GOOD $BAD; do "$AD" secret delete $n >/dev/null 2>&1; done
node -e "process.stdout.write(require('crypto').randomBytes(5).toString('hex')+'\n')" | "$AD" secret set $GOOD --app $APP --site 127.0.0.1 --stdin >/dev/null
node -e "process.stdout.write(require('crypto').randomBytes(5).toString('hex')+'\n')" | "$AD" secret set $BAD --app $APP --site login.example.com --stdin >/dev/null
# A fresh profile each run: a browser reopening a used profile may ignore the close request below.
rm -rf "${D:?}/profile-site-$BR"
"$AD" launch "$BROWSER_EXE" --no-attach $EXTRA $LANGX "--arg=--user-data-dir=$WD\\profile-site-$BR" --arg=--no-first-run "--arg=http://127.0.0.1:$PORT/login-unnamed.html" --timeout 30000 >/dev/null
W=""
for i in $(seq 1 15); do W=$("$AD" list-windows --app $APP | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const w=o.data.find(w=>/Unnamed login/.test(w.title));console.log(w?w.id:'')})"); [ -n "$W" ] && break; sleep 1; done
echo "window $W"
BAR=$("$AD" snapshot --window-id $W | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);let h='';(function w(n){if(!n||h)return;if(n.role==='textfield'&&n.value&&/127\.0\.0\.1/.test(n.value))h=n.name;(n.children||[]).forEach(w)})(o.data&&o.data.tree);console.log(h)})")
echo "the browser names its address bar: \"$BAR\""
PW=""
for i in $(seq 1 15); do
  "$AD" snapshot --window-id $W > "$F"
  PW=$(node -e "const o=require(process.argv[1]);let h='';(function w(n){if(!n||h)return;if(n.role==='textfield'&&(n.states||[]).includes('secure')&&n.ref_id)h=n.ref_id;(n.children||[]).forEach(w)})(o.data&&o.data.tree);console.log(h)" "$(cygpath -w "$F")")
  [ -n "$PW" ] && break; sleep 1
done
show() { node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);console.log('   ',o.ok?'ok':('refused: '+o.error.message))})"; }
echo "== a secret bound to another site (must be refused)"; "$AD" secret fill "$PW" $BAD | show
echo "== a secret bound to 127.0.0.1"; "$AD" secret fill "$PW" $GOOD | show
"$AD" wait --text "pw-len: 10" --window-id $W --timeout 5000 | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);console.log(o.ok?'    -> PASS (the page has 10 characters)':'    -> FAIL ('+o.error.message+')')})"
for n in $GOOD $BAD; do "$AD" secret delete $n >/dev/null; done
kill $SERVER 2>/dev/null
powershell -NoProfile -Command "Get-CimInstance Win32_Process -Filter \"Name='$APP'\" | Where-Object { \$_.CommandLine -like '*profile-site-$BR*' -and \$_.CommandLine -notlike '*--type=*' } | ForEach-Object { \$id = \$_.ProcessId; taskkill.exe /PID \$id | Out-Null; Wait-Process -Id \$id -Timeout 10 -ErrorAction SilentlyContinue; if (Get-Process -Id \$id -ErrorAction SilentlyContinue) { Stop-Process -Id \$id -Force } }"
