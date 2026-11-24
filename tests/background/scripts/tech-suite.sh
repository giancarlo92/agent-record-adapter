#!/usr/bin/env bash
# Part of Agent Record Adapter (see NOTICE).
# The whole background suite, one app per Windows UI technology, on the hidden work desktop and on
# the person's own desktop (apps behind their windows), then the Chromium page tests. Run it after
# any change to the background routes, instead of checking one app:
#   Win32 classic, WinForms, WPF, Java Swing, Electron, UWP   tech-matrix-test.sh
#   winit/egui                                                 wordcraft-keys-test.sh, deckcraft-keys-test.sh
#   WinUI 3                                                    paint-keys-test.sh
#   Chromium pages (dedicated Chrome)                         sgh-like-test.sh, nav-test.sh, secret-site-test.sh
# Only the summary lines are shown; every test closes only what it started.
# Usage: tech-suite.sh <agent-record.exe>
set -u
AD="$1"
T="$(cd "$(dirname "$0")" && pwd)"
sum() { grep -E -- "^==|-> |FOREGROUND|stayed|unchanged|^window" | sed 's/^/  /'; }
for where in hidden person; do
  echo "######## tech-matrix, $where"; timeout 600 bash "$T/tech-matrix-test.sh" "$AD" "$where" 2>&1 | sum
  for app in wordcraft deckcraft paint; do
    echo "######## $app, $where"; timeout 300 bash "$T/$app-keys-test.sh" "$AD" "$([ "$where" = person ] && echo person)" 2>&1 | sum
  done
done
echo "######## Chromium pages"
printf "  page checks: "; timeout 300 bash "$T/sgh-like-test.sh" "$AD" native 2>&1 | grep -E "PASS|FAIL|ERR" | sed -E 's/ *-> (PASS|FAIL).*/\1/; s/ *ERR.*/ERR/' | sort | uniq -c | tr '\n' ' '; echo
printf "  navigate: "; timeout 300 bash "$T/nav-test.sh" "$AD" native 2>&1 | grep -E "PASS|FAIL" | tr '\n' ' '; echo
printf "  secret in a Spanish Chrome: "; timeout 300 bash "$T/secret-site-test.sh" "$AD" --lang=es 2>&1 | grep -E "PASS|FAIL|refused" | tr '\n' ' '; echo
echo "ALL DONE"
