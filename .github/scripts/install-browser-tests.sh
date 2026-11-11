#!/usr/bin/env bash
set -euo pipefail

bash "$(dirname "${BASH_SOURCE[0]}")/setup-ubuntu-apt.sh"

pnpm exec playwright install --with-deps chromium
