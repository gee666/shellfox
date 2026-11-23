#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "$0")/.."
host=${LINUX_SSH_HOST:-omarchy-red}
slot=${LINUX_SYNC_SLOT:-}
[[ -z $slot || $slot =~ ^[a-z0-9-]+$ ]] || { echo 'LINUX_SYNC_SLOT must be lowercase letters, digits and dashes' >&2; exit 1; }
ssh_options=(-o BatchMode=yes -o ConnectTimeout=10)
if [[ -n ${LINUX_SSH_CONFIG:-} ]]; then ssh_options+=(-F "$LINUX_SSH_CONFIG"); fi

# Include current tracked and non-ignored untracked files. Skip deletions.
mkdir -p tmp
archive=$(mktemp tmp/linux-sync.XXXXXXXX)
trap 'rm -f -- "$archive"' EXIT
git ls-files -z --cached --others --exclude-standard --deduplicate |
  while IFS= read -r -d '' path; do
    case /$path/ in */target/*|*/tmp/*|*/node_modules/*|*/.git/*) continue ;; esac
    if [[ -f $path || -L $path ]]; then printf '%s\0' "$path"; fi
  done | tar -czf "$archive" --null -T -

# Give Cargo fresh Linux mtimes even if the Windows clock lags.
ssh "${ssh_options[@]}" -- "$host" 'timeout 300 bash -c '\''
    set -euo pipefail
    cd /home/oira666/shellfox-ghostti-migration.8lza8G
    dest=src-current
    if [[ -n ${1:-} ]]; then
      mkdir -p "tmp/$1"
      dest="tmp/$1/src"
    fi
    stage=$(mktemp -d tmp/src-current.XXXXXXXX)
    trap "rm -rf -- $stage" EXIT
    tar -mxzf - -C "$stage"
    rm -rf -- "$dest"
    mv -T -- "$stage" "$dest"
  '\'' sync' "$slot" < "$archive"
