#!/bin/sh
# Shellfox updater for Linux (.deb installs) and macOS (Shellfox.app).
# Usage: shellfox update [--check]
# Test overrides: SHELLFOX_UPDATE_API (latest-release JSON URL), SHELLFOX_UPDATE_REPO (owner/name),
# SHELLFOX_PLISTBUDDY (PlistBuddy path).
set -u

REPO=${SHELLFOX_UPDATE_REPO:-gee666/shellfox}
API=${SHELLFOX_UPDATE_API:-https://api.github.com/repos/$REPO/releases/latest}
RELEASES_URL=https://github.com/$REPO/releases
PLISTBUDDY=${SHELLFOX_PLISTBUDDY:-/usr/libexec/PlistBuddy}
TMPDIR_CREATED=

say() { printf '%s\n' "$*"; }
die() { printf 'Shellfox update: %s\n' "$*" >&2; exit 1; }
cleanup() { if [ -n "$TMPDIR_CREATED" ]; then rm -rf "$TMPDIR_CREATED"; fi; }
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

usage() {
  say 'Usage: shellfox update [--check]'
  say 'Downloads and installs the latest published Shellfox release.'
  say '  --check   only report whether a newer version is available'
}

# is_newer CANDIDATE CURRENT: succeeds when CANDIDATE is a higher dotted numeric
# version. A leading "v" and any -/+ suffix are ignored; non-numeric parts count as 0.
version_core() { v=${1#v}; v=${v%%[-+]*}; printf '%s' "$v"; }
is_newer() {
  a=$(version_core "$1"); b=$(version_core "$2")
  while [ -n "$a" ] || [ -n "$b" ]; do
    pa=${a%%.*}; pb=${b%%.*}
    case $a in *.*) a=${a#*.} ;; *) a= ;; esac
    case $b in *.*) b=${b#*.} ;; *) b= ;; esac
    pa=${pa%%[!0-9]*}; pb=${pb%%[!0-9]*}
    pa=${pa:-0}; pb=${pb:-0}
    if [ "$pa" -gt "$pb" ]; then return 0; fi
    if [ "$pa" -lt "$pb" ]; then return 1; fi
  done
  return 1
}

# http_get URL OUTFILE [quiet]: returns 0 on success, 44 when the server answered 404, 1 otherwise.
http_get() {
  url=$1; out=$2; quiet=${3:-}
  if command -v curl >/dev/null 2>&1; then
    if [ -n "$quiet" ]; then
      status=$(curl -sSL --connect-timeout 15 --max-time 60 -H 'User-Agent: shellfox-update' -H 'Accept: application/vnd.github+json' -o "$out" -w '%{http_code}' "$url") || return 1
    else
      status=$(curl -SL --progress-bar --connect-timeout 15 --max-time 1800 -H 'User-Agent: shellfox-update' -o "$out" -w '%{http_code}' "$url") || return 1
    fi
    case $status in 404) return 44 ;; 4??|5??) return 1 ;; esac
    return 0
  elif command -v wget >/dev/null 2>&1; then
    errfile="$out.err"
    if wget -S -O "$out" --user-agent='shellfox-update' --timeout=60 "$url" 2>"$errfile"; then rm -f "$errfile"; return 0; fi
    if grep -q ' 404 ' "$errfile" 2>/dev/null; then rm -f "$errfile"; return 44; fi
    rm -f "$errfile"
    return 1
  fi
  die 'curl or wget is required.'
}

json_string() { # json_string KEY FILE: first string value of KEY
  grep -o "\"$1\"[[:space:]]*:[[:space:]]*\"[^\"]*\"" "$2" | head -n 1 | sed 's/^[^:]*:[[:space:]]*"\(.*\)"$/\1/'
}
asset_url() { # asset_url NAME FILE: browser_download_url whose file name is NAME
  grep -o '"browser_download_url"[[:space:]]*:[[:space:]]*"[^"]*"' "$2" | sed 's/^[^:]*:[[:space:]]*"\(.*\)"$/\1/' | while IFS= read -r candidate; do
    case $candidate in */"$1") printf '%s\n' "$candidate"; break ;; esac
  done
}

CHECK=0
for arg in "$@"; do
  case $arg in
    --check) CHECK=1 ;;
    --help|-h) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done

OS=$(uname -s 2>/dev/null || echo unknown)
case $OS in
  Linux)
    command -v dpkg-query >/dev/null 2>&1 || die "this Shellfox is not installed from the .deb package. Download the latest release from $RELEASES_URL"
    CURRENT=$(dpkg-query -W -f='${Version}' shellfox 2>/dev/null) || CURRENT=
    [ -n "$CURRENT" ] || die "this Shellfox is not installed from the .deb package, so it cannot update itself. Download the latest release from $RELEASES_URL"
    ;;
  Darwin)
    if [ -n "${SHELLFOX_APP_EXECUTABLE:-}" ]; then
      EXE=$SHELLFOX_APP_EXECUTABLE
      case $EXE in */Contents/MacOS/*) BUNDLE=${EXE%/Contents/MacOS/*} ;; *) BUNDLE=/Applications/Shellfox.app ;; esac
    else
      BUNDLE=/Applications/Shellfox.app
      EXE=$BUNDLE/Contents/MacOS/Shellfox
    fi
    PLIST=$BUNDLE/Contents/Info.plist
    [ -f "$PLIST" ] || die "Shellfox.app was not found at $BUNDLE. Download the latest release from $RELEASES_URL"
    BUNDLE_ID=$("$PLISTBUDDY" -c 'Print :CFBundleIdentifier' "$PLIST" 2>/dev/null) || BUNDLE_ID=
    [ "$BUNDLE_ID" = local.shellfox ] || die "$BUNDLE is not a Shellfox app bundle. Download the latest release from $RELEASES_URL"
    CURRENT=$("$PLISTBUDDY" -c 'Print :CFBundleShortVersionString' "$PLIST" 2>/dev/null) || CURRENT=
    [ -n "$CURRENT" ] || die "could not read the installed version from $PLIST"
    ;;
  *) die "unsupported system '$OS'. Download the latest release from $RELEASES_URL" ;;
esac

TMPDIR_CREATED=$(mktemp -d "${TMPDIR:-/tmp}/shellfox-update.XXXXXX") || die 'could not create a temporary directory.'
case $TMPDIR_CREATED in /*) ;; *) TMPDIR_CREATED=$PWD/$TMPDIR_CREATED ;; esac
WORK=$TMPDIR_CREATED
chmod 755 "$WORK"

say "Installed Shellfox version: $CURRENT"
http_get "$API" "$WORK/release.json" quiet
rc=$?
if [ "$rc" -eq 44 ]; then say 'No published Shellfox release yet.'; exit 0; fi
[ "$rc" -eq 0 ] || die "could not query the latest release ($API). Check your network connection, or see $RELEASES_URL"
TAG=$(json_string tag_name "$WORK/release.json")
[ -n "$TAG" ] || die "the release information did not contain a version. See $RELEASES_URL"
LATEST=${TAG#v}

if ! is_newer "$LATEST" "$CURRENT"; then
  say "Shellfox is up to date ($CURRENT)."
  exit 0
fi
if [ "$CHECK" -eq 1 ]; then
  say "Shellfox $LATEST is available (installed: $CURRENT). Run: shellfox update"
  exit 0
fi
say "Updating Shellfox $CURRENT -> $LATEST"

fetch_asset() { # fetch_asset NAME: downloads to $WORK/NAME
  url=$(asset_url "$1" "$WORK/release.json")
  [ -n "$url" ] || die "release $TAG has no file named $1. See $RELEASES_URL"
  say "Downloading $1"
  http_get "$url" "$WORK/$1" || die "download failed: $url"
  [ -s "$WORK/$1" ] || die "downloaded file is empty: $1"
}

case $OS in
  Linux)
    ARCH=$(dpkg --print-architecture 2>/dev/null) || ARCH=
    case $ARCH in amd64|arm64) ;; *) die "unsupported architecture '${ARCH:-unknown}' (amd64 and arm64 packages are published). See $RELEASES_URL" ;; esac
    DEB=shellfox_${LATEST}_${ARCH}.deb
    fetch_asset "$DEB"
    chmod 644 "$WORK/$DEB"
    SUDO=
    if [ "$(id -u 2>/dev/null || echo 1)" != 0 ]; then
      command -v sudo >/dev/null 2>&1 || die "root access is required to install the package. Re-run as root or install sudo."
      SUDO=sudo
    fi
    if command -v apt-get >/dev/null 2>&1; then
      # shellcheck disable=SC2086
      $SUDO apt-get install -y "$WORK/$DEB" || die 'apt-get could not install the package.'
    else
      # shellcheck disable=SC2086
      $SUDO dpkg -i "$WORK/$DEB" || die 'dpkg could not install the package.'
    fi
    ;;
  Darwin)
    ARCHS=$(lipo -archs "$EXE" 2>/dev/null) || ARCHS=$(uname -m)
    case " $ARCHS " in
      *" arm64 "*" x86_64 "*|*" x86_64 "*" arm64 "*) if [ "$(uname -m)" = arm64 ]; then ARCH=arm64; else ARCH=x64; fi ;;
      *" arm64 "*|*" arm64e "*) ARCH=arm64 ;;
      *) ARCH=x64 ;;
    esac
    ZIP=Shellfox-darwin-${ARCH}-${LATEST}.zip
    fetch_asset "$ZIP"
    mkdir "$WORK/extracted" || die 'could not create a temporary directory.'
    ditto -x -k "$WORK/$ZIP" "$WORK/extracted" || die 'could not extract the downloaded archive.'
    NEWAPP=$WORK/extracted/Shellfox.app
    [ -d "$NEWAPP" ] || die 'the downloaded archive does not contain Shellfox.app.'
    xattr -dr com.apple.quarantine "$NEWAPP" 2>/dev/null || true
    PARENT=$(dirname "$BUNDLE")
    SUDO=
    if [ ! -w "$PARENT" ]; then
      command -v sudo >/dev/null 2>&1 || die "$PARENT is not writable and sudo is unavailable."
      SUDO=sudo
      say "Administrator access is needed to modify $PARENT."
    fi
    STAGED=$PARENT/.Shellfox.app.new.$$
    OLD=$PARENT/.Shellfox.app.old.$$
    $SUDO rm -rf "$STAGED" "$OLD"
    $SUDO ditto "$NEWAPP" "$STAGED" || { $SUDO rm -rf "$STAGED"; die 'could not copy the new version into place.'; }
    $SUDO mv "$BUNDLE" "$OLD" || { $SUDO rm -rf "$STAGED"; die 'could not move the old version aside.'; }
    if ! $SUDO mv "$STAGED" "$BUNDLE"; then
      $SUDO mv "$OLD" "$BUNDLE"
      $SUDO rm -rf "$STAGED"
      die 'could not install the new version; the previous version was restored.'
    fi
    $SUDO rm -rf "$OLD"
    ;;
esac

say "Shellfox $LATEST installed."
say "Restart Shellfox to use $LATEST (quitting Shellfox closes its terminals)."
exit 0
