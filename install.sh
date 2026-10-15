#!/usr/bin/env bash
# Installs Private Music on macOS or Linux from the latest GitHub release.
#
#   ./install.sh                 install or update
#   ./install.sh --uninstall     remove it
#
# The repository is private, so a token is needed: set GITHUB_TOKEN, or log in
# with the GitHub CLI (`gh auth login`) and it is picked up from there.
# Settings (all optional): PM_REPO, PM_VERSION (e.g. v0.2.0), PM_API (for tests).
set -euo pipefail

REPO="${PM_REPO:-decoder-dev/PrivateMusicDesktop}"
API="${PM_API:-https://api.github.com}"
VERSION="${PM_VERSION:-latest}"
APP_NAME="Private Music"

say() { printf '\033[1m==>\033[0m %s\n' "$*" >&2; }
die() { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "'$1' is required but not installed."; }

need curl
case "$(uname -s)" in
  Darwin) OS=macos ;;
  Linux) OS=linux ;;
  *) die "this script is for macOS and Linux; on Windows use install.ps1." ;;
esac

# ---------------------------------------------------------------- uninstall
if [ "${1:-}" = "--uninstall" ]; then
  if [ "$OS" = macos ]; then
    rm -rf "/Applications/$APP_NAME.app" "$HOME/Applications/$APP_NAME.app"
  else
    if command -v dpkg >/dev/null 2>&1 && dpkg -s private-music >/dev/null 2>&1; then
      say "removing the package (needs administrator rights)"
      if [ "$(id -u)" -eq 0 ]; then apt-get remove -y private-music; else sudo apt-get remove -y private-music; fi
    fi
    rm -f "$HOME/.local/bin/private-music" "$HOME/.local/share/applications/private-music.desktop"
    rm -rf "$HOME/.local/share/private-music"
  fi
  say "removed. Saved sign-in stays in your system keyring until you sign out in the app."
  exit 0
fi

# --------------------------------------------------------------------- token
TOKEN="${GITHUB_TOKEN:-${GH_TOKEN:-}}"
if [ -z "$TOKEN" ] && command -v gh >/dev/null 2>&1; then
  TOKEN="$(gh auth token 2>/dev/null || true)"
fi
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
# The token goes to curl through a private file, never on its command line,
# where any local user could read it in the process list.
AUTH=()
if [ -n "$TOKEN" ]; then
  (umask 077; printf 'Authorization: Bearer %s\n' "$TOKEN" > "$TMP/auth")
  AUTH=(-H "@$TMP/auth")
fi

api() { curl -fsSL "${AUTH[@]+"${AUTH[@]}"}" -H "Accept: application/vnd.github+json" "$@"; }

# ------------------------------------------------------------------- release
say "looking up the release ($VERSION)"
if [ "$VERSION" = latest ]; then URL="$API/repos/$REPO/releases/latest"; else URL="$API/repos/$REPO/releases/tags/$VERSION"; fi
RELEASE="$(api "$URL")" || die "could not read the release. For a private repository set GITHUB_TOKEN (or run 'gh auth login'); a draft release must be published first."

TAG="$(printf '%s\n' "$RELEASE" | sed -n 's/^ *"tag_name": *"\([^"]*\)".*/\1/p' | head -n1)"
[ -n "$TAG" ] || die "the release has no tag."

# Pairs of "asset id<TAB>file name", from the pretty-printed JSON.
ASSETS="$(printf '%s\n' "$RELEASE" | awk '
  /"url": *"[^"]*\/releases\/assets\/[0-9]+"/ { match($0, /assets\/[0-9]+/); id = substr($0, RSTART + 7, RLENGTH - 7) }
  /^ *"name": *"/ && id != "" { n = $0; sub(/^ *"name": *"/, "", n); sub(/".*/, "", n); print id "\t" n; id = "" }')"

asset_id() { printf '%s\n' "$ASSETS" | awk -F'\t' -v pat="$1" '$2 ~ pat { print $1; exit }'; }
asset_name() { printf '%s\n' "$ASSETS" | awk -F'\t' -v pat="$1" '$2 ~ pat { print $2; exit }'; }

fetch() { # pattern -> downloads the matching asset into $TMP, prints its path
  local id name
  id="$(asset_id "$1")"; name="$(asset_name "$1")"
  [ -n "$id" ] || die "the release has no file matching $1."
  say "downloading $name"
  curl -fSL --progress-bar "${AUTH[@]+"${AUTH[@]}"}" -H "Accept: application/octet-stream" \
    -o "$TMP/$name" "$API/repos/$REPO/releases/assets/$id"
  printf '%s' "$TMP/$name"
}

verify() { # path
  local sums id name want got
  id="$(asset_id '^SHA256SUMS\.txt$')"
  [ -n "$id" ] || die "the release has no SHA256SUMS.txt; refusing to install an unverified file."
  curl -fsSL "${AUTH[@]+"${AUTH[@]}"}" -H "Accept: application/octet-stream" -o "$TMP/SHA256SUMS.txt" \
    "$API/repos/$REPO/releases/assets/$id"
  name="$(basename "$1")"
  want="$(awk -v n="$name" '$2 == n || $2 == "*" n { print $1; exit }' "$TMP/SHA256SUMS.txt")"
  [ -n "$want" ] || die "$name is not listed in SHA256SUMS.txt."
  if command -v sha256sum >/dev/null 2>&1; then got="$(sha256sum "$1" | awk '{print $1}')"; else got="$(shasum -a 256 "$1" | awk '{print $1}')"; fi
  [ "$want" = "$got" ] || die "checksum mismatch for $name; the download is damaged or was tampered with."
  say "checksum ok"
}

# --------------------------------------------------------------------- macOS
install_macos() {
  local dmg mount dest app
  dmg="$(fetch '\.dmg$')"; verify "$dmg"
  mount="$TMP/mount"; mkdir -p "$mount"
  hdiutil attach -nobrowse -quiet -readonly -mountpoint "$mount" "$dmg"
  app="$(find "$mount" -maxdepth 1 -name '*.app' | head -n1)"
  [ -n "$app" ] || { hdiutil detach -quiet "$mount" || true; die "no application inside the disk image."; }
  dest="/Applications"
  [ -w "$dest" ] || { dest="$HOME/Applications"; mkdir -p "$dest"; }
  say "copying to $dest"
  rm -rf "$dest/$APP_NAME.app"
  cp -R "$app" "$dest/$APP_NAME.app"
  hdiutil detach -quiet "$mount" || true
  # The build is not notarised; a file fetched by this script is trusted by
  # you, so drop the quarantine flag that would otherwise block the first launch.
  xattr -dr com.apple.quarantine "$dest/$APP_NAME.app" 2>/dev/null || true
  say "installed: $dest/$APP_NAME.app"
}

# --------------------------------------------------------------------- Linux
install_linux() {
  case "$(uname -m)" in x86_64 | amd64) ;; *) die "only x86-64 Linux builds are published." ;; esac
  if [ -z "${PM_FORCE_APPIMAGE:-}" ] && command -v dpkg >/dev/null 2>&1 && command -v apt-get >/dev/null 2>&1; then
    local deb
    deb="$(fetch '\.deb$')"; verify "$deb"
    say "installing the package (needs administrator rights)"
    if [ "$(id -u)" -eq 0 ]; then apt-get install -y "$deb"; else need sudo; sudo apt-get install -y "$deb"; fi
    say "installed. Start it from the application menu or run: private-music-desktop"
    return
  fi
  local image bin share
  image="$(fetch '\.AppImage$')"; verify "$image"
  bin="$HOME/.local/bin"; share="$HOME/.local/share"
  mkdir -p "$bin" "$share/private-music" "$share/applications"
  install -m 0755 "$image" "$share/private-music/PrivateMusic.AppImage"
  ln -sf "$share/private-music/PrivateMusic.AppImage" "$bin/private-music"
  cat > "$share/applications/private-music.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=$APP_NAME
Comment=Unofficial VK Music player
Exec=$share/private-music/PrivateMusic.AppImage
Terminal=false
Categories=AudioVideo;Audio;Music;
DESKTOP
  say "installed: $share/private-music/PrivateMusic.AppImage (also as 'private-music' in $bin)"
  case ":$PATH:" in *":$bin:"*) ;; *) say "add $bin to your PATH to start it from a terminal" ;; esac
}

say "Private Music $TAG for $OS"
if [ "$OS" = macos ]; then install_macos; else install_linux; fi
say "done"
