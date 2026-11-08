#!/usr/bin/env bash
# Puts the Gradle distribution named by a wrapper properties file where the wrapper looks for it,
# trying services.gradle.org (with retries), the GitHub release it redirects to, and then two independent
# mirrors (services.gradle.org only redirects to GitHub, so a GitHub outage takes both down). The SHA-256
# from the properties file is always checked, so a mirror cannot substitute a different build.
set -euo pipefail
props="${1:?usage: $0 <gradle-wrapper.properties>}"
url="$(sed -n 's/^distributionUrl=//p' "$props" | sed 's/\\:/:/g')"
sum="$(sed -n 's/^distributionSha256Sum=//p' "$props")"
[ -n "$url" ] && [ -n "$sum" ] || { echo "distributionUrl or distributionSha256Sum missing in $props" >&2; exit 2; }
name="$(basename "$url" .zip)"
hash="$(python3 -c 'import hashlib,sys
n=int.from_bytes(hashlib.md5(sys.argv[1].encode()).digest(),"big");d="0123456789abcdefghijklmnopqrstuvwxyz";s=""
while n: n,r=divmod(n,36); s=d[r]+s
print(s or "0")' "$url")"
dir="${GRADLE_USER_HOME:-$HOME/.gradle}/wrapper/dists/$name/$hash"
zip="$dir/$name.zip"
if [ -f "$zip" ] && echo "$sum  $zip" | sha256sum -c --quiet 2>/dev/null; then echo "Gradle $name already present"; exit 0; fi
mkdir -p "$dir"
version="${name#gradle-}"; version="${version%-bin}"; version="${version%-all}"
for source in "$url" \
  "https://github.com/gradle/gradle-distributions/releases/download/v$version/$name.zip" \
  "https://mirrors.cloud.tencent.com/gradle/$name.zip" \
  "https://distfiles.macports.org/gradle/$name.zip"; do
  if curl -fsSL --retry 4 --retry-delay 5 --retry-all-errors --connect-timeout 20 --max-time 600 -o "$zip.part" "$source" \
     && echo "$sum  $zip.part" | sha256sum -c --quiet; then
    mv "$zip.part" "$zip"; echo "Gradle $name from $source"; exit 0
  fi
  echo "::warning::Gradle download from $source failed or did not match the checksum"
  rm -f "$zip.part"
done
echo "::warning::could not prefetch $name; the Gradle wrapper will try itself"
