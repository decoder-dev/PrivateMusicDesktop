#!/usr/bin/env bash
# Runs install.sh against a fake GitHub API: release lookup, asset download,
# checksum check (good and tampered), AppImage install and uninstall.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d)"
trap 'kill "${server:-0}" 2>/dev/null || true; rm -rf "$work"' EXIT

mkdir -p "$work/assets"
printf '#!/bin/sh\necho private-music-ok\n' > "$work/assets/PrivateMusic_9.9.9_linux-amd64.AppImage"
printf 'not really a deb\n' > "$work/assets/PrivateMusic_9.9.9_linux-amd64.deb"
(cd "$work/assets" && sha256sum * > SHA256SUMS.txt)

cat > "$work/server.py" <<'PY'
import http.server, json, os, sys
root, port = sys.argv[1], int(sys.argv[2])
names = sorted(os.listdir(os.path.join(root, "assets")))
class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self):
        if self.headers.get("Authorization") != "Bearer test-token":
            self.send_response(404); self.end_headers(); return
        if self.path.endswith("/releases/latest") or self.path.endswith("/releases/tags/v9.9.9"):
            base = f"http://127.0.0.1:{port}/repos/o/r/releases/assets"
            assets = [{"url": f"{base}/{i + 1}", "id": i + 1, "node_id": "x", "name": n, "size": 1} for i, n in enumerate(names)]
            body = json.dumps({"tag_name": "v9.9.9", "draft": False, "assets": assets}, indent=2).encode()
            self.send_response(200); self.send_header("Content-Type", "application/json")
        elif "/releases/assets/" in self.path:
            index = int(self.path.rsplit("/", 1)[1]) - 1
            body = open(os.path.join(root, "assets", names[index]), "rb").read()
            if os.environ.get("TAMPER") and names[index].endswith("AppImage"):
                body += b"x"
            self.send_response(200); self.send_header("Content-Type", "application/octet-stream")
        else:
            self.send_response(404); self.end_headers(); return
        self.send_header("Content-Length", str(len(body))); self.end_headers(); self.wfile.write(body)
http.server.ThreadingHTTPServer(("127.0.0.1", port), H).serve_forever()
PY

port=$((20000 + RANDOM % 20000))
start_server() { # $1: "tamper" to serve a corrupted AppImage
  if [ -n "${server:-}" ]; then kill "$server" 2>/dev/null || true; wait "$server" 2>/dev/null || true; fi
  if [ "${1:-}" = tamper ]; then TAMPER=1 python3 "$work/server.py" "$work" "$port" & else python3 "$work/server.py" "$work" "$port" & fi
  server=$!
  for _ in $(seq 1 50); do curl -s "http://127.0.0.1:$port/" >/dev/null 2>&1 && return; sleep 0.1; done
  echo "FAIL: fake API did not start"; exit 1
}
start_server tamper

export PM_API="http://127.0.0.1:$port" PM_REPO=o/r PM_FORCE_APPIMAGE=1 HOME="$work/home"
mkdir -p "$HOME"
unset GH_TOKEN

echo "-- no token: refused with a hint"
if GITHUB_TOKEN= "$here/install.sh" >"$work/out" 2>&1; then echo "FAIL: installed without a token"; exit 1; fi
grep -q "GITHUB_TOKEN" "$work/out" || { cat "$work/out"; echo "FAIL: no hint about the token"; exit 1; }

echo "-- tampered download: checksum mismatch"
if GITHUB_TOKEN=test-token "$here/install.sh" >"$work/out" 2>&1; then echo "FAIL: installed a tampered file"; exit 1; fi
grep -q "checksum mismatch" "$work/out" || { cat "$work/out"; echo "FAIL: mismatch not reported"; exit 1; }
[ ! -e "$HOME/.local/share/private-music/PrivateMusic.AppImage" ] || { echo "FAIL: tampered file was installed"; exit 1; }
start_server

echo "-- good download: installs the AppImage"
GITHUB_TOKEN=test-token "$here/install.sh" >"$work/out" 2>&1 || { cat "$work/out"; echo "FAIL: install failed"; exit 1; }
"$HOME/.local/bin/private-music" | grep -q private-music-ok || { echo "FAIL: installed program does not run"; exit 1; }
grep -q "^Exec=$HOME/.local/share/private-music/PrivateMusic.AppImage" "$HOME/.local/share/applications/private-music.desktop" || { echo "FAIL: desktop entry"; exit 1; }

echo "-- specific version and reinstall over it"
GITHUB_TOKEN=test-token PM_VERSION=v9.9.9 "$here/install.sh" >/dev/null 2>&1 || { echo "FAIL: pinned version"; exit 1; }

echo "-- uninstall"
"$here/install.sh" --uninstall >/dev/null 2>&1
[ ! -e "$HOME/.local/bin/private-music" ] && [ ! -e "$HOME/.local/share/applications/private-music.desktop" ] || { echo "FAIL: uninstall left files"; exit 1; }
echo "install.sh: all checks passed"
