#!/usr/bin/env bash
# Starts the built app in its CI smoke mode: it opens the VK sign-in window
# exactly as the button does and quits once VK's page has loaded. Fails when
# the page never loads (as WebView2 did when the window came from a
# synchronous command). Usage: scripts/smoke-login.sh <path to the app binary>
set -euo pipefail
binary="$1"
report="$(mktemp -d)/smoke.txt"
export PM_SMOKE_LOGIN="$report"
case "$(uname -s)" in
  Linux) runner=(dbus-run-session -- xvfb-run -a) ;;
  *) runner=() ;;
esac
set +e
timeout 150 "${runner[@]+"${runner[@]}"}" "$binary"
status=$?
set -e
echo "app exited with $status"
cat "$report" 2>/dev/null || echo "(no report)"
for log in "$HOME/.local/share/com.dec.privatemusic2.desktop/logs/private-music.log" \
           "$HOME/Library/Logs/com.dec.privatemusic2.desktop/private-music.log"; do
  [ -f "$log" ] && { echo "--- $log"; cat "$log"; }
done
grep -q '^loaded ' "$report"
