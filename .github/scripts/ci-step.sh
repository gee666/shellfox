#!/usr/bin/env bash
# Usage: ci-step.sh <name> <command...>
# Runs a CI command, streams its output, and on failure publishes the last lines as
# an error annotation (annotations are readable without access to the full logs).
set -uo pipefail
name=$1; shift
mkdir -p tmp
log="tmp/ci-${name}.log"
"$@" 2>&1 | tee "$log"
code=${PIPESTATUS[0]}
if [ "$code" -ne 0 ]; then
  # GitHub truncates long annotation messages from the end: send only the last ~3.5 KB.
  message=$(tr -d '\r' < "$log" | sed -e 's/\x1b\[[0-9;]*[A-Za-z]//g' | tail -c 3500 | sed -e 's/%/%25/g' | awk '{ printf "%s%%0A", $0 }')
  echo "::error title=${name} failed (${RUNNER_OS:-local} ${RUNNER_ARCH:-})::${message}"
fi
exit "$code"
