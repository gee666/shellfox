#!/usr/bin/env bash
set -euo pipefail

# Change only disposable GitHub Ubuntu runners, never developer machines.
if [[ "${GITHUB_ACTIONS:-}" != "true" || "${RUNNER_OS:-}" != "Linux" ]]; then
  exit 0
fi
source /etc/os-release
if [[ "$ID" != "ubuntu" ]]; then
  exit 0
fi

arch="$(dpkg --print-architecture)"
case "$arch" in
  amd64) mirror="${SHELLFOX_UBUNTU_MIRROR:-https://mirrors.edge.kernel.org/ubuntu}" ;;
  arm64) mirror="${SHELLFOX_UBUNTU_PORTS_MIRROR:-https://mirrors.mit.edu/ubuntu-ports}" ;;
  *) echo "Unsupported Ubuntu archive architecture: $arch" >&2; exit 1 ;;
esac

sudo "$(command -v node)" "$(dirname "${BASH_SOURCE[0]}")/setup-ubuntu-apt.mjs" "$mirror"
