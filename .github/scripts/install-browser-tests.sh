#!/usr/bin/env bash
set -euo pipefail

# Only disposable GitHub Ubuntu runners. Keep the signed Ubuntu archive metadata,
# but avoid the Azure mirror's slow font downloads during Playwright dependency setup.
if [[ "${RUNNER_OS:-}" == "Linux" && -f /etc/apt/sources.list.d/ubuntu.sources ]]; then
  sudo python3 - "${SHELLFOX_UBUNTU_MIRROR:-https://archive.ubuntu.com/ubuntu}" <<'PY'
from pathlib import Path
from urllib.parse import urlparse
import sys

mirror = sys.argv[1].rstrip('/')
parsed = urlparse(mirror)
if parsed.scheme != 'https' or not parsed.hostname or parsed.username or parsed.password or parsed.query or parsed.fragment:
    raise SystemExit('SHELLFOX_UBUNTU_MIRROR must be an HTTPS Ubuntu archive origin/path.')
source = Path('/etc/apt/sources.list.d/ubuntu.sources')
original = source.read_text()
updated = original.replace('http://azure.archive.ubuntu.com/ubuntu', mirror).replace('https://azure.archive.ubuntu.com/ubuntu', mirror)
if updated != original:
    source.write_text(updated)
    print('Using the configured Ubuntu archive mirror for browser test dependencies.')
PY
fi

pnpm exec playwright install --with-deps chromium
