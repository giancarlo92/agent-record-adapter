#!/usr/bin/env bash
# Part of Agent Record Adapter (see NOTICE). Ported from the desktop-recorder-mcp fork.
# The SGH structures (sgh-like.html): DR-04 menu, DR-03 row icons, DR-07 unlabelled fields, DR-08 PrimeNG radios.
# Any Chromium browser: BROWSER_EXE=<its program> (Chrome by default).
# The browser is closed as a person closes it (no "quit unexpectedly" prompt next time).
# Usage: sgh-like-test.sh <agent-record.exe> <cdp|native>
set -u
AD="$1"; KIND="$2"
export AGENT_DESKTOP_HOME="$LOCALAPPDATA/DesktopRecorder/tmp/js-state" TEMP="$LOCALAPPDATA/DesktopRecorder/tmp" TMP="$LOCALAPPDATA/DesktopRecorder/tmp"
BROWSER_EXE="${BROWSER_EXE:-C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe}"; APP=$(basename "${BROWSER_EXE//\\//}"); BR=${APP%.exe}
# A new Edge profile signs into the Windows Microsoft account and syncs its extensions: not in a test profile.
EXTRA=""; [ "$BR" = msedge ] && EXTRA="--arg=--disable-features=msImplicitSignin"
D="$USERPROFILE/AprendeReplica-e2e/enter-test"; WD=$(cygpath -w "$D"); F="$D/sl-$KIND.json"
CDP=""; [ "$KIND" = cdp ] && CDP="--cdp"
# A fresh profile each run: a browser reopening a used profile may ignore the close request below.
rm -rf "${D:?}/profile-sl-$KIND-$BR"
L=$("$AD" launch "$BROWSER_EXE" $CDP --no-attach $EXTRA "--arg=--user-data-dir=$WD\\profile-sl-$KIND-$BR" --arg=--no-first-run "--arg=file:///$(cygpath -m "$D/sgh-like.html")" --timeout 30000)
W=$(echo "$L" | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);console.log(o.data.window?o.data.window.id:'')})")
# The window by its title: a browser reopening a profile may replace the first window launch saw (Brave does).
for i in $(seq 1 15); do T=$("$AD" list-windows --app $APP | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const w=o.data.find(w=>/SGH-like/.test(w.title));console.log(w?w.id:'')})"); [ -n "$T" ] && { W=$T; break; }; sleep 1; done
echo "##### $KIND window $W"
short() { node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);console.log('   ',o.ok?((o.data.disposition||{}).delivery||'')+' '+JSON.stringify((o.data.steps||[]).map(x=>x.label)):('ERR '+o.error.code+' '+o.error.message+' '+JSON.stringify((o.error.details||{}).kind||'')))})"; }
snap() { "$AD" snapshot --window-id $W > "$F"; }
status() { for i in $(seq 1 10); do snap; got=$(node -e "const o=require(process.argv[1]);let s='';(function w(n){if(!n||s)return;if(n.role==='statictext'&&/^estado:/.test(n.name||''))s=n.name;(n.children||[]).forEach(w)})(o.data.tree);console.log(s)" "$(cygpath -w "$F")"); [ "$got" = "estado: $1" ] && { echo "    -> PASS ($got)"; return; }; sleep 0.5; done; echo "    -> FAIL (page says: $got; wanted: estado: $1)"; }
open_state() { for i in $(seq 1 10); do snap; got=$(node -e "const o=require(process.argv[1]);let s='hidden';(function w(n){if(!n)return;if(n.name==='Usuarios'&&!(n.states||[]).includes('offscreen'))s='shown';(n.children||[]).forEach(w)})(o.data.tree);console.log(s)" "$(cygpath -w "$F")"); [ "$got" = "$1" ] && { echo "    -> PASS (Usuarios $got)"; return; }; sleep 0.5; done; echo "    -> FAIL (Usuarios $got, wanted $1)"; }
q() { node -e "$1" "$(cygpath -w "$F")" "${@:2}"; }
pick_option() { q "const o=require(process.argv[1]);let h='';(function w(n){if(!n||h)return;if(n.role==='option'&&/^Personal/.test(n.name||'')&&n.ref_id)h=n.ref_id;(n.children||[]).forEach(w)})(o.data.tree);console.log(h)"; }
pick_group() { q "const o=require(process.argv[1]);const path=[];let h='';(function w(n){if(!n||h)return;path.push(n);if(n.role==='statictext'&&n.name==='Personal'){const opt=path.find(x=>x.role==='option');const i=path.indexOf(opt);const g=path.slice(i+1).find(x=>x.ref_id);h=g?g.ref_id:''}(n.children||[]).forEach(w);path.pop()})(o.data.tree);console.log(h)"; }
for i in $(seq 1 20); do snap; n=$(q "const o=require(process.argv[1]);let c=0;(function w(n){if(!n)return;if(n.name==='Personal')c++;(n.children||[]).forEach(w)})(o.data.tree);console.log(c)"); [ "$n" -gt 0 ] && break; sleep 1; done
echo "== DR-04 click the option item (named list item)"; O=$(pick_option); "$AD" click "$O" | short; open_state shown
snap; O=$(pick_option); echo "== DR-04 click it again (close)"; "$AD" click "$O" | short; open_state hidden
snap; G=$(pick_group); echo "== DR-04 click the unnamed group (open)"; "$AD" click "$G" | short; open_state shown
snap; G=$(pick_group); echo "== DR-04 click the unnamed group again (close)"; "$AD" click "$G" | short; open_state hidden
echo "== DR-03 rows: refs of the icons"; snap
q "const o=require(process.argv[1]);const path=[];(function w(n){if(!n)return;path.push(n);if(n.role==='row'||n.role==='cell'){};(n.children||[]).forEach(w);path.pop()})(o.data.tree);" >/dev/null
EDIT=$(q "const o=require(process.argv[1]);let out=[];(function w(n){if(!n)return;if(n.role==='row'&&(n.children||[]).some(c=>c.name==='QA26A1197')){(function g(m){if(!m)return;if(m.role==='group'&&m.ref_id)out.push(m.ref_id);(m.children||[]).forEach(g)})(n)}(n.children||[]).forEach(w)})(o.data.tree);console.log(out.join(','))")
echo "    refs in row QA26A1197: $EDIT"; E=$(echo "$EDIT" | cut -d, -f1); T=$(echo "$EDIT" | cut -d, -f2)
echo "== DR-03 click first icon (Editar)"; "$AD" click "$E" | short; status "editar QA26A1197"
snap; EDIT=$(q "const o=require(process.argv[1]);let out=[];(function w(n){if(!n)return;if(n.role==='row'&&(n.children||[]).some(c=>c.name==='QA26A1197')){(function g(m){if(!m)return;if(m.role==='group'&&m.ref_id)out.push(m.ref_id);(m.children||[]).forEach(g)})(n)}(n.children||[]).forEach(w)})(o.data.tree);console.log(out.join(','))"); T=$(echo "$EDIT" | cut -d, -f2)
echo "== DR-03 click second icon (estado)"; "$AD" click "$T" | short; status "estado QA26A1197"
echo "== DR-07 unlabelled fields"; snap
FIELDS=$(q "const o=require(process.argv[1]);const r=[];let d=0;(function w(n){if(!n)return;if(n.role==='document')d++;if(d&&n.role==='textfield'&&!n.name&&n.ref_id)r.push(n.ref_id);(n.children||[]).forEach(w);if(n.role==='document')d--})(o.data.tree);console.log(r.join(' '))")
k=0; for R in $FIELDS; do echo "  field $k ($R) set-value"; "$AD" set-value "$R" "valor$k" | short; status "campo $k=valor$k"; k=$((k+1)); done
echo "== DR-08 PrimeNG radios"; snap; R=$(q "const o=require(process.argv[1]);let h='';(function w(n){if(!n||h)return;if(n.role==='radiobutton'&&n.name==='Pago directo'&&n.ref_id)h=n.ref_id;(n.children||[]).forEach(w)})(o.data.tree);console.log(h)")
echo "  click Pago directo ($R)"; "$AD" click "$R" | short; status "pago directo"
snap; R=$(q "const o=require(process.argv[1]);let h='';(function w(n){if(!n||h)return;if(n.role==='radiobutton'&&n.name==='Pago de procedimiento'&&n.ref_id)h=n.ref_id;(n.children||[]).forEach(w)})(o.data.tree);console.log(h)")
echo "  click Pago de procedimiento ($R)"; "$AD" click "$R" | short; status "pago proc"
powershell -NoProfile -Command "Get-CimInstance Win32_Process -Filter \"Name='$APP'\" | Where-Object { \$_.CommandLine -like '*profile-sl-$KIND-$BR*' -and \$_.CommandLine -notlike '*--type=*' } | ForEach-Object { \$id = \$_.ProcessId; taskkill.exe /PID \$id | Out-Null; Wait-Process -Id \$id -Timeout 10 -ErrorAction SilentlyContinue; if (Get-Process -Id \$id -ErrorAction SilentlyContinue) { Stop-Process -Id \$id -Force } }"
