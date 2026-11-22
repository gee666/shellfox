#!/bin/sh
pid=$1; work=$2; package=$3; executable=$4; expected=$5; control=$6; relaunch=$7
exec >"$work/install.log" 2>&1
failed() {
  if [ -x /usr/bin/zenity ]; then /usr/bin/zenity --error --text='Shellfox update failed or was cancelled. Reopen Shellfox to try again.'; fi
  if [ "$relaunch" = 1 ]; then "$executable" >/dev/null 2>&1 & fi
  exit 1
}
actual=$(/usr/bin/sha256sum -- "$package") || exit 1
[ "${actual%% *}" = "$expected" ] || exit 1
: > "$control/ready" || exit 1
tries=0
while [ ! -f "$control/commit" ]; do
  [ ! -f "$control/cancel" ] || exit 1
  kill -0 "$pid" 2>/dev/null || exit 1
  tries=$((tries + 1))
  [ "$tries" -le 12000 ] || exit 1
  sleep 0.05
done
[ ! -f "$control/cancel" ] || exit 1
: > "$control/committed" || exit 1
tries=0
while kill -0 "$pid" 2>/dev/null; do
  [ ! -f "$control/cancel" ] || exit 1
  tries=$((tries + 1))
  [ "$tries" -le 1200 ] || exit 1
  sleep 0.1
done
[ ! -f "$control/cancel" ] || exit 1
actual=$(/usr/bin/sha256sum -- "$package") || failed
[ "${actual%% *}" = "$expected" ] || failed
if /usr/bin/pkexec /usr/bin/apt-get install -y "$package"; then
  if [ "$relaunch" = 1 ]; then "$executable" >/dev/null 2>&1 & fi
  rm -rf -- "$work"
else
  failed
fi
