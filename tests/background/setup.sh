#!/usr/bin/env bash
# Part of Agent Record Adapter (see NOTICE). Ported from the desktop-recorder-mcp fork.
# Copies the test pages to the folder the scripts work in (pages, outputs and
# browser profiles of their own): %USERPROFILE%\AprendeReplica-e2e\enter-test.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
work="$USERPROFILE/AprendeReplica-e2e/enter-test"
mkdir -p "$work/files"
cp "$here"/pages/*.html "$work/"
[ -f "$work/files/sample.csv" ] || printf 'id,name\n1,Ana\n' > "$work/files/sample.csv"
[ -f "$work/files/second.csv" ] || printf 'x\n' > "$work/files/second.csv"
echo "pages copied to $work"
