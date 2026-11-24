#!/usr/bin/env bash
# Part of Agent Record Adapter (see NOTICE).
# The same background actions on one app per Windows UI technology, so a change is checked against
# all of them at once rather than app by app:
#   Win32 classic   Character Map (charmap.exe)
#   WinForms        classic-menu.exe  } built by tests/background/apps/build-test-apps.ps1, the same
#   WPF             wpf-text.exe      } windows as classic-menu.ps1 and wpf-text.ps1
#   Java Swing      tests/background/apps/swing-text.java (no accessibility tree at all)
#   Electron        VS Code, with a profile of its own in a test folder
#   UWP             Calculator (only with "person": a packaged app the system starts opens on the
#                   desktop on screen, never on the work desktop)
# winit/egui (WordCraft, DeckCraft), WinUI 3 (Paint) and Chromium pages have tests of their own;
# tech-suite.sh runs them all.
# Each app: its window is deactivated (another window takes the activation and goes away, as the
# person's windows take it), then typed text, a key, and a click, each read back from the app.
# hidden (default): on the hidden work desktop. person: on the person's own desktop, the apps behind
# their windows, and the window in front must be the same at the end.
# Only the processes this test started are closed (by pid; VS Code with its child processes).
# Usage: tech-matrix-test.sh <agent-record.exe> [hidden|person]
set -u
AD="$1"; WHERE="${2:-hidden}"
export AGENT_DESKTOP_HOME="$LOCALAPPDATA/DesktopRecorder/tmp/js-state"
[ "$WHERE" = person ] || export AGENT_DESKTOP_WORK_DESKTOP=artest
HERE="$(cd "$(dirname "$0")" && pwd)"; APPS="$(cd "$HERE/../apps" && pwd)"
TMP="$LOCALAPPDATA/DesktopRecorder/tmp/tech-matrix"; mkdir -p "$TMP"
PS='C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe'
FG() { powershell -NoProfile -ExecutionPolicy Bypass -File "$HERE/foreground.ps1"; }
BEFORE=$(FG)
BUILT=$(powershell -NoProfile -ExecutionPolicy Bypass -File "$APPS/build-test-apps.ps1" -Out "$(cygpath -w "$TMP/apps")" | tr -d '\r')
PASS=0; FAIL=0; LIMIT=0; PIDS=""; TREES=""; WINS=""
# Whether a window of this test is in front (the person may move between their own windows meanwhile).
ours_in_front() { local F; F=$(FG); for w in $WINS; do [ "$w" = "w-${F%% *}" ] && return 0; done; return 1; }
ok() { PASS=$((PASS+1)); echo "    -> PASS ($1)"; }
no() { FAIL=$((FAIL+1)); echo "    -> FAIL ($1)"; }
limit() { LIMIT=$((LIMIT+1)); echo "    -> LIMIT ($1)"; }
is() { [ "$1" = "$2" ] && ok "$3: $1" || no "$3: got $1, wanted $2"; }
r() { node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{try{const o=JSON.parse(s);process.stdout.write(o.ok?'ok':'ERR('+o.error.code+': '+o.error.message.slice(0,90)+')')}catch(e){process.stdout.write('?')}})"; }
# launch writes to a file: a pipe would stay open as long as the launched app runs.
launch() { "$AD" launch "$@" --timeout 40000 >"$TMP/launch.json" 2>/dev/null; node -e "try{const o=JSON.parse(require('fs').readFileSync(process.argv[1],'utf8'));console.log((o.data&&o.data.pid)||'')}catch(e){console.log('')}" "$TMP/launch.json"; }
# win <app exe> <pid or ""> <title regex>: "<id> <active|deactivated>"
win() { "$AD" list-windows --app "$1" | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const w=(o.data||[]).find(w=>(!process.argv[1]||w.pid==process.argv[1])&&new RegExp(process.argv[2]).test(w.title||''));console.log(w?w.id+' '+(w.is_focused?'active':'deactivated'):'')})" "$2" "$3"; }
title() { "$AD" list-windows --app "$1" | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const w=(o.data||[]).find(w=>w.id===process.argv[1]);console.log(w?w.title:'')})" "$2"; }
# find <window> <field>=<regex> ... -> "<ref>\t<value>" of the first element whose fields match
# (fields: role, name, id = the automation id).
find() { local W="$1"; shift; "$AD" snapshot --window-id "$W" | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const want=process.argv.slice(1).map(a=>{const i=a.indexOf('=');return[a.slice(0,i),new RegExp(a.slice(i+1))]});let hit=null;(function w(n){if(!n||hit)return;const f={role:n.role||'',name:n.name||'',id:(n.native_id&&n.native_id.value)||''};if(want.every(([k,re])=>re.test(f[k])))hit=(n.ref_id||'-')+'\t'+(n.value||'');(n.children||[]).forEach(w)})(o.data&&o.data.tree);console.log(hit||'')})" "$@"; }
# nameof <window> <field>=<regex> ...: the name of the first element whose fields match.
nameof() { local W="$1"; shift; "$AD" snapshot --window-id "$W" | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const o=JSON.parse(s);const want=process.argv.slice(1).map(a=>{const i=a.indexOf('=');return[a.slice(0,i),new RegExp(a.slice(i+1))]});let hit=null;(function w(n){if(!n||hit!==null)return;const f={role:n.role||'',name:n.name||'',id:(n.native_id&&n.native_id.value)||''};if(want.every(([k,re])=>re.test(f[k])))hit=n.name||'';(n.children||[]).forEach(w)})(o.data&&o.data.tree);console.log(hit||'')})" "$@"; }
deactivate() { [ "$WHERE" = person ] && return; "$AD" launch "$PS" --arg=-NoProfile --arg=-Command --arg="Start-Sleep -Seconds 2" --timeout 30000 >/dev/null 2>&1; sleep 4; }
# The usual edit: "abc", Backspace, "xyz" typed into the window (where its focus is) -> "abxyz".
edit() { local W="$1"
  printf "    type abc "; "$AD" type "$W" "abc" | r
  printf ", backspace "; "$AD" press backspace --window-id "$W" | r
  printf ", type xyz "; "$AD" type "$W" "xyz" | r; echo
}

echo "== Win32 classic: Character Map"
P=$(launch 'C:\Windows\System32\charmap.exe'); PIDS="$PIDS $P"; sleep 2
read W ST < <(win charmap.exe "$P" '.')
# Its dialog keeps the keyboard focus on the character grid: text goes into the field by its ref.
deactivate; read W ST < <(win charmap.exe "$P" '.'); WINS="$WINS $W"; echo "    window $W ($ST)"
read REF V < <(find "$W" id='^104$'); printf "    type abxyz into the field "; "$AD" type "$REF" "abxyz" | r; echo
read REF V < <(find "$W" id='^104$'); is "$V" abxyz "field"
read B _ < <(find "$W" id='^103$'); printf "    click Select "; "$AD" click "$B" | r; echo; sleep 1
read REF V < <(find "$W" id='^104$'); is "$V" 'abxyz!' "Select added the selected character"
printf "    right (in the grid, where the focus is) "; "$AD" press right --window-id "$W" | r; echo; sleep 1
is "$(nameof "$W" id='^501$')" "U+0022: Quotation Mark" "the grid moved to the next character"

echo "== WinForms: classic-menu.exe"
P=$(launch "$BUILT\classic-menu.exe"); PIDS="$PIDS $P"; sleep 3
read W ST < <(win classic-menu.exe "$P" 'Classic menu test')
read REF V < <(find "$W" role='^textfield$'); "$AD" focus "$REF" >/dev/null 2>&1
deactivate; read W ST < <(win classic-menu.exe "$P" 'Classic menu test'); WINS="$WINS $W"; echo "    window $W ($ST)"
edit "$W"
read REF V < <(find "$W" role='^textfield$'); is "$V" abxyz "field"
read B _ < <(find "$W" role='^button$' name='^Apply$'); printf "    click Apply "; "$AD" click "$B" | r; echo; sleep 1
STATUS=$(nameof "$W" name='^status:')
is "$STATUS" "status: applied abxyz" "Apply"
printf "    ctrl+s "; "$AD" press ctrl+s --window-id "$W" | r; echo; sleep 1
STATUS=$(nameof "$W" name='^status:')
is "$STATUS" "status: saved" "Ctrl+S ran File > Save"

echo "== WPF: wpf-text.exe"
P=$(launch "$BUILT\wpf-text.exe"); PIDS="$PIDS $P"; sleep 4
deactivate; read W ST < <(win wpf-text.exe "$P" 'WPF text test'); WINS="$WINS $W"; echo "    window $W ($ST)"
edit "$W"
is "$(title wpf-text.exe "$W")" "WPF text test: abxyz" "title shows the box"

echo "== Java Swing: swing-text.java (no accessibility tree)"
JAVA=$(command -v java >/dev/null && cygpath -w "$(command -v java)")
if [ -z "$JAVA" ]; then echo "    (no java: skipped)"; else
P=$(launch "$JAVA" --arg="$(cygpath -w "$APPS/swing-text.java")"); PIDS="$PIDS $P"; sleep 3
for i in 1 2 3 4 5; do read W ST < <(win java.exe "$P" '^Swing text test'); [ -n "$W" ] && break; sleep 1; done
deactivate; read W ST < <(win java.exe "$P" '^Swing text test'); WINS="$WINS $W"; echo "    window $W ($ST)"
edit "$W"
is "$(title java.exe "$W")" "Swing text test: abxyz" "title shows the field"
printf "    tab, space "; "$AD" press tab --window-id "$W" | r; "$AD" press space --window-id "$W" | r; echo; sleep 1
is "$(title java.exe "$W")" "Swing text test: applied abxyz" "Apply by keyboard"
# The button by a click at its place (content 10,60 120x30; the title bar is 31 px at 100 %).
SF=$("$AD" screenshot --window-id "$W" "$(cygpath -w "$TMP/swing.png")" | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{try{console.log(JSON.parse(s).data.scale_factor||1)}catch(e){console.log(1)}})")
printf "    shift+tab, type q "; "$AD" press shift+tab --window-id "$W" | r; "$AD" type "$W" "q" | r; echo; sleep 1
is "$(title java.exe "$W")" "Swing text test: abxyzq" "back in the field"
printf "    click-at Apply "; "$AD" click-at "$(node -e "console.log(Math.round(70*$SF))")" "$(node -e "console.log(Math.round(106*$SF))")" --window-id "$W" | r; echo; sleep 1
[ "$(title java.exe "$W")" = "Swing text test: applied abxyzq" ] && ok "Apply by a click" || limit "AWT takes a click only where the real pointer is: --headed"
fi

echo "== Electron: VS Code with a test profile"
CODE="$LOCALAPPDATA/Programs/Microsoft VS Code/Code.exe"
if [ ! -f "$CODE" ]; then echo "    (no VS Code: skipped)"; else
NOTE="$TMP/note-$WHERE.txt"; printf '' >"$NOTE"; mkdir -p "$TMP/code-data" "$TMP/code-ext"
P=$(launch "$(cygpath -w "$CODE")" --arg="--user-data-dir=$(cygpath -w "$TMP/code-data")" --arg="--extensions-dir=$(cygpath -w "$TMP/code-ext")" --arg=--disable-extensions --arg=--disable-workspace-trust --arg=--skip-release-notes --arg=--new-window --arg="$(cygpath -w "$NOTE")"); TREES="$TREES $P"
for i in $(seq 1 15); do read W ST < <(win Code.exe "$P" "^note-$WHERE.txt"); [ -n "$W" ] && break; sleep 1; done
sleep 3; deactivate; read W ST < <(win Code.exe "$P" "^note-$WHERE.txt"); WINS="$WINS $W"; echo "    window $W ($ST)"
printf "    type "; "$AD" type "$W" "hola electron" | r
printf ", ctrl+s "; "$AD" press ctrl+s --window-id "$W" | r; echo; sleep 2
is "$(cat "$NOTE")" "hola electron" "saved file (once, not doubled)"
fi

if [ "$WHERE" = person ]; then
echo "== UWP: Calculator (opens behind, on the desktop on screen)"
EARLIER=" $(powershell -NoProfile -Command "(Get-Process CalculatorApp -ErrorAction SilentlyContinue).Id -join ' '") "
launch 'C:\Windows\System32\calc.exe' >/dev/null
for i in $(seq 1 15); do read W ST < <(win CalculatorApp.exe "" '.'); [ -n "$W" ] && break; sleep 1; done
NEWCALC=$(powershell -NoProfile -Command "(Get-Process CalculatorApp -ErrorAction SilentlyContinue).Id -join ' '")
WINS="$WINS $W"; ours_in_front && no "the Calculator came to the front at its launch" || ok "the launch kept the person's window in front ($(FG))"; WINS="$WINS $W"; echo "    window $W ($ST)"
read C _ < <(find "$W" id='^clearButton$'); printf "    click Clear "; "$AD" click "$C" | r; echo; sleep 1
is "$(nameof "$W" id='^CalculatorResults$')" "Display is 0" "cleared"
printf "    type 12+3= "; "$AD" type "$W" "12+3=" | r; echo; sleep 1
is "$(nameof "$W" id='^CalculatorResults$')" "Display is 15" "typed (+ and = need Shift on some keyboards)"
printf "    press 4, *, 2, = "; for K in 4 '*' 2 '='; do "$AD" press "$K" --window-id "$W" | r; done; echo; sleep 1
is "$(nameof "$W" id='^CalculatorResults$')" "Display is 8" "pressed 4 * 2 ="
for K in num7Button plusButton num8Button equalButton; do read B _ < <(find "$W" id="^$K\$"); "$AD" click "$B" >/dev/null 2>&1; done; sleep 1
is "$(nameof "$W" id='^CalculatorResults$')" "Display is 15" "clicks 7 + 8 ="
for p in $NEWCALC; do echo "$EARLIER" | grep -q " $p " || PIDS="$PIDS $p"; done
fi

echo "== $PASS passed, $FAIL failed, $LIMIT known limits"
[ "$WHERE" = person ] && { ours_in_front && echo "    FOREGROUND TAKEN by a test window: $(FG)" || echo "    the person's window stayed in front (before: $BEFORE, now: $(FG))"; }
for p in $PIDS; do [ -n "$p" ] && powershell -NoProfile -Command "Stop-Process -Id $p -Force -ErrorAction SilentlyContinue"; done
for p in $TREES; do [ -n "$p" ] && taskkill //T //F //PID "$p" >/dev/null 2>&1; done
