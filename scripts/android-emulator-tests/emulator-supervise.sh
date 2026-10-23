#!/usr/bin/env bash
# Detached parent keeps the emulator alive across CI steps and records its real exit status.
set -uo pipefail
exit_file="$1"
pid_file="$2"
shift 2
"$@" &
child=$!
printf '%s\n' "$child" > "$pid_file"
wait "$child"
status=$?
printf 'status=%s time=%s\n' "$status" "$(date -u +%FT%TZ)" > "$exit_file"
exit "$status"
