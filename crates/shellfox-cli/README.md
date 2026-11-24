# shellfox-cli

The `shellfox` command for desktop and headless installs. Its commands, flags, JSON and exit codes match Shellfox 0.9 (see [docs/cli.md](../../docs/cli.md) and [docs/headless.md](../../docs/headless.md)). The same binary is the host process for `host run`, the desktop's host (`--desktop-host`, used internally) and the PTY supervisor.

```sh
shellfox --user-data /srv/shellfox-data host run --json
shellfox session create /srv/project --title Backend --json
shellfox session list --json
shellfox terminals attach TAB_UUID
shellfox host stop --yes --json
```

`--user-data DIR` selects a 0.9-style data root. The host keeps its state in `DIR/rust-v1/host` and imports 0.9 data on first start.

## Development builds

Packaged builds find their data root and Tailscale helper on their own. Source builds have no bundled helper, so put an explicit selection before the command:

```powershell
$sel = @('--data-dir', $data, '--helper', $helper, '--helper-sha256', $sha)
& target/debug/shellfox-cli.exe @sel host run
& target/debug/shellfox-cli.exe @sel session create . --title Demo --json
```

`--data-dir` names the Rust host directory itself, with no 0.9 import, legacy-host checks or desktop cold start. `--helper-sha256` must come from a trusted helper build, never from hashing a file you found. Remote test flags (`--remote-state-dir`, `--remote-hostname`, `--remote-control-url`) go in the same prefix.

With an explicit selection, development operator commands that 0.9 did not have also work and print raw JSON: `tab list|create|rename|close`, `attach TAB [--takeover]`, `settings get|set`, `host power status|on|off`, `host network status|on|off|login|cancel|continue|peer PEER OP`. Scripts and tests use them; users don't need them.

## Checks

```text
cargo test --locked -p shellfox-cli
```

Integration tests start a real host on isolated data under the project `tmp/` with the development helper `tmp/dev-helper/shellfox-tailscale.exe`. `session_create` creates sessions through the real CLI and lists them. `desktop_handoff` checks update forwarding and the desktop cold start with a stub desktop.
