#!/usr/bin/env bash
set -euo pipefail

output="${1:?Specify the directory containing the generated DEB}"
shopt -s nullglob
packages=("$output"/privatemusic_*.deb)
if (( ${#packages[@]} != 1 )); then
  echo "Expected exactly one Private Music DEB in $output" >&2
  exit 1
fi

staging=$(mktemp -d "$output/.prepare-deb.XXXXXX")
trap 'rm -rf -- "$staging"' EXIT
dpkg-deb --raw-extract "${packages[0]}" "$staging/package"

# jpackage registers the shortcut in postinst. xdg-desktop-menu requires writable
# system directories, which do not exist on a clean minimal Debian or Ubuntu install.
install -d -m 0755 "$staging/package/usr/share/applications" "$staging/package/usr/share/desktop-directories"

dpkg-deb --root-owner-group --build "$staging/package" "$staging/updated.deb"
mv -f -- "$staging/updated.deb" "${packages[0]}"
