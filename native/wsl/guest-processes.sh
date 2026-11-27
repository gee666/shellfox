# The fallback for guest-processes.py when Python fails, with the same output.
# GNU grep -z reads NUL-separated environment entries, so a newline inside a
# value can't fake a marker. base64 keeps whitespace, quotes, empty argv slots
# and control characters intact.
export LC_ALL=C PATH=/usr/bin:/bin
for tool in cat id readlink base64 grep head wc; do
  command -v "$tool" >/dev/null 2>&1 || exit 69
done
printf 'x\000' | grep -az '^x$' >/dev/null || exit 69
uid=$(id -u) || exit 69
[ "$uid" = "$(id -ru)" ] || exit 69
stat_fields() {
  rest=${1##*) }
  set -f; set -- $rest; set +f
  [ "$#" -ge 20 ] || return 1
  state=$1; parent=$2; shift 19; ticks=$1
  case "$parent:$ticks" in *[!0-9:]*|:*) return 1;; esac
}
printf 'SHELLFOX_GUEST_PROCESSES_1\n'
n=0
for d in /proc/[0-9]*; do
  n=$((n+1)); [ "$n" -le 20000 ] || exit 70
  before=$(cat "$d/stat" 2>/dev/null) || { [ ! -d "$d" ] || printf 'E\n'; continue; }
  stat_fields "$before" || { printf 'E\n'; continue; }
  case "$state" in Z|X|x) continue;; esac
  pp=$parent; start=$ticks
  real=''; effective=''
  while read -r label a b rest; do
    if [ "$label" = 'Uid:' ]; then real=$a; effective=$b; break; fi
  done 2>/dev/null < "$d/status"
  [ "$real" = "$uid" ] && [ "$effective" = "$uid" ] || continue
  [ -r "$d/environ" ] || continue
  size=$(head -c 1048577 "$d/environ" 2>/dev/null | wc -c) || continue
  [ "$size" -le 1048576 ] || { printf 'E\n'; continue; }
  marker=$(head -c 1048576 "$d/environ" 2>/dev/null | grep -az '^SHELLFOX_TERMINAL_MARKER=' | base64 -w0)
  [ -n "$marker" ] || continue
  ok=1
  path=$(readlink "$d/exe" 2>/dev/null) && exe=$(printf '%s' "${path% (deleted)}" | base64 -w0) || { exe='-'; ok=0; }
  args=$({ head -c 65537 "$d/cmdline" 2>/dev/null && printf '\0001' || printf '\0000'; } | base64 -w0)
  name=${before#*(}; name=${name%)*}; name=$(printf '%s' "$name" | base64 -w0)
  after=$(cat "$d/stat" 2>/dev/null) || continue
  stat_fields "$after" || continue
  case "$state" in Z|X|x) continue;; esac
  [ "$start" = "$ticks" ] && [ "$pp" = "$parent" ] || ok=0
  printf 'R\t%s\t%s\t%s\t%s\t%s\n' "$ok" "$exe" "$args" "$marker" "$name"
done
printf 'END\n'
