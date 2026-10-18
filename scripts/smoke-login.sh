#!/usr/bin/env bash
# Проверяет настоящее окно входа VK в собранном приложении и завершает его
# после загрузки страницы. Если страница не появилась, проверка завершается ошибкой.
# Так обнаруживается, например, блокировка WebView2 при синхронном открытии окна.
# Запуск: scripts/smoke-login.sh <путь к исполняемому файлу приложения>
set -euo pipefail
binary="$1"
report="$(mktemp -d)/smoke.txt"
export PM_SMOKE_LOGIN="$report"
case "$(uname -s)" in
  Linux) runner=(dbus-run-session -- xvfb-run -a) ;;
  *) runner=() ;;
esac
# У macOS нет `timeout`, поэтому фоновый сторожевой таймер останавливает зависшее приложение.
set +e
"${runner[@]+"${runner[@]}"}" "$binary" &
app=$!
( sleep "${SMOKE_TIMEOUT:-150}"; echo "the app did not finish in time"; kill "$app" ) 2>/dev/null &
watchdog=$!
wait "$app"
status=$?
kill "$watchdog" 2>/dev/null
set -e
echo "app exited with $status"
cat "$report" 2>/dev/null || echo "(no report)"
for log in "$HOME/.local/share/com.dec.privatemusic2.desktop/logs/private-music.log" \
           "$HOME/Library/Logs/com.dec.privatemusic2.desktop/private-music.log"; do
  [ -f "$log" ] && { echo "--- $log"; cat "$log"; }
done
grep -q '^loaded ' "$report"
