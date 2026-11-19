#!/bin/sh
pid=$1; work=$2; bundle=$3; stage=$4; control=$5; relaunch=$6
exec >"$work/install.log" 2>&1
failed() {
  /usr/bin/osascript -e 'display alert "Shellfox update failed" message "The previous app was kept. Reopen Shellfox and try again."' || true
  if [ "$relaunch" = 1 ]; then /usr/bin/open -n "$bundle"; fi
  exit 1
}
/usr/bin/codesign --verify --deep --strict "$stage/Shellfox.app" || exit 1
: > "$control/ready" || exit 1
tries=0
while [ ! -f "$control/commit" ]; do
  [ ! -f "$control/cancel" ] || exit 1
  kill -0 "$pid" 2>/dev/null || exit 1
  tries=$((tries + 1))
  [ "$tries" -le 3000 ] || exit 1
  sleep 0.2
done
[ ! -f "$control/cancel" ] || exit 1
: > "$control/committed" || exit 1
tries=0
while kill -0 "$pid" 2>/dev/null; do
  [ ! -f "$control/cancel" ] || exit 1
  tries=$((tries + 1))
  [ "$tries" -le 120 ] || exit 1
  sleep 1
done
[ ! -f "$control/cancel" ] || exit 1
old="$stage/previous.app"
new="$stage/Shellfox.app"
/usr/bin/codesign --verify --deep --strict "$new" || failed
if mv -- "$bundle" "$old"; then
  if mv -- "$new" "$bundle"; then
    if [ "$relaunch" = 1 ]; then /usr/bin/open -n "$bundle"; fi
    rm -rf -- "$stage" "$work"
    exit 0
  fi
  if ! mv -- "$old" "$bundle"; then
    /usr/bin/osascript -e 'display alert "Shellfox rollback failed" message "The previous app is in the .Shellfox-update backup folder beside the installation. Restore it manually."' || true
    exit 1
  fi
fi
failed
