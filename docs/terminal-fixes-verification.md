# Terminal fixes verification

## Regression coverage

- Mobile history: `mobile/src/terminal/terminal-page.test.mjs` opens restored scrollback, swipes upward through it, and checks that live output does not force it back to the bottom. Alternate-screen swipes do not send invented input.
- Request pressure: `mobile/src/client/terminal-load.test.ts` delays transport replies by one second while sending 1,500 input, resize and visibility events. Desktop `src/renderer/remote-pressure.test.ts` holds host replies for 60 seconds through a 1,500-event burst. Tests check bounded concurrency, coalescing, connection changes and no ownership heartbeats.
- Exclusive ownership: `tests/e2e/terminal-sizing.spec.ts` uses a real PTY and the desktop renderer. It checks a blank blocked view, explicit Take it, denied old-owner resize and automatic return after disconnect. Broker, renderer, mobile and CLI tests cover stale replay, ownership epochs, hidden output, and cleanup across local IPC, headless RPC and remote HTTP.
- Startup: `tests/e2e/embedded-acceptance.spec.ts` restarts with a recently used legacy session and verifies its last activity and unsettled state. Service and inactivity tests also cover embedded sessions, explicit settlement and expiry.
- CLI attachment: `tests/e2e/cli-attach-tty.spec.ts` creates sessions/tabs through the window API, then launches the built CLI in an external PTY against an isolated desktop host using existing tab UUIDs only. It checks no CLI creation, required IDs, both attach namespace spellings, Ctrl+T takeover, shell cwd/input/output and Ctrl+] detachment. A pre-sized 90x40 Node TUI checks same-grid repaint without manually resizing the external TTY. The staged `node-pty` addon runs in a separate bundled-Node driver, not the Playwright worker, with JSON-line commands/events over stdio pipes. It records real PTY data and exit codes, driver errors/stderr and exit status in `external-tty-evidence.json`, `same-grid-repaint.json` and `same-grid-driver.json`.
- CLI display: table tests cover narrow widths, full IDs/paths, Unicode display width, colors and escaped metadata. Help tests cover aliases.
- Discovery: probes identify Shellfox without pairing. Tests cover bounded probes, cache deadlines, newly appearing hosts, offline saved hosts, failed discovery followed by successful pairing, and legacy connected-host compatibility.
- Approval and screen lock: tests cover full queues, same-attempt suspend/resume/cancel, late native responses, headless request delivery and 100 rapid screen changes. Android bounds pending-pair completion with a wake lock; iOS uses a finite background task. Host requests remain pending for one hour without a heartbeat.
- Session sorting: desktop and mobile call `src/shared/session-order.ts`. Mobile UI tests check the same pins, statuses, busy transitions and stable ties.

## Verified runs

Windows x64 verification used Node 24.22.1 and Electron 44.5.1. Desktop and headless build stamps both match source digest `4cda55ae5f6be6f303eaecda7767552cea32e1a7252ae38a2d55e352a7b3a1c7`.

| Check | Result |
| --- | --- |
| `pnpm typecheck` | Desktop and mobile passed |
| `pnpm test:unit` | Default fork pool: 3,025 passed, 52 platform/opt-in skips |
| Playwright E2E | All 44 non-skipped cases passed across the full run and targeted reruns; 3 skipped |
| External CLI/native TUI | Existing-ID attachment, input, takeover, detach and same-grid repaint passed; two viewers stayed stable for 31 seconds without renewal or output feedback |
| Native .NET | 65 passed |
| Electron PTY and SQLite | Spawn, resize, replay, confirmed exit, persistence, migration and rollback checks passed |
| Packaging contracts | 48 passed, including checksum and secret checks |
| Embedded networking | Tailscale and mobile Go suites passed; 14 shared-health tests include a real isolated SDK recovery fixture |
| Headless artifact | Built and passed its offline CLI/SQLite/PTY smoke without a UI runtime |
| Android | Universal test APK assembled, linted and signer-verified; not installed on a phone |
| Linux/WSL | 433 focused TypeScript tests passed; real WSL mounted-drive and guest-filesystem cwd/dispatch checks passed |
| Native cross-compilation | Linux amd64 and macOS arm64 helpers compiled |

The first broad E2E run exposed stale command/settings expectations and a deleted-terminal attachment regression. The expectations were migrated to canonical commands. New attachments and Take now require live saved metadata; an existing owner may still acknowledge its final authorized output after exit. All failed cases passed on rerun. The final critical eight-case run and the isolated CLI/WSL run also passed.

Local logs are under `tmp/`: `issues-unit-final.log`, `issues-e2e-final.log`, `issues-e2e-migrated-final.log`, `issues-wsl-cold-final.log`, `issues-e2e-core-fixed-runtime.log`, `issues-cli-wsl-isolated-final.log`, `issues-mobile-build-final.log`, and the `issues-nethealth-*` logs. Playwright attachments contain the native identity, working-directory, repaint and exit evidence. These ignored artifacts are local evidence, not committed credentials or release assets.

## Windows runtime correction

Intermittent native test-worker crashes were traced through official Node symbols to libuv 1.51.0's uninitialized Windows version structure, not a JavaScript test failure. The upstream fix is [libuv #5107](https://github.com/libuv/libuv/commit/aabb7651de73ec2f1a74361ca3430eed1a62e402). Bundled CLI/headless runtimes and workflow pins now use Node 24.22.1 with libuv 1.52.1, with official checksums pinned for every supported archive and Windows native build asset. Electron already used a fixed libuv. The final unit run used the default fork pool without instrumentation or a pool workaround; no global Node installation or authentication was changed.

## Module boundaries

The existing `service.ts`, `backend.ts` and `terminal-client.ts` state machines remain above 350 lines. Their shared transaction queues, owned handles and ownership epochs must stay visible together for lifecycle review. The control broker, lifecycle policy, renderer surface, query filter and OS-specific termination helpers are separate modules; further splitting these coordinating classes would scatter the safety transitions. New regression groups use separate files where they do not depend on a large shared fixture.

## Limits

Mobile scrollback is bounded at 3,000 rows. The host's 256 KiB attachment budget can reduce restored history further. Alternate-screen applications control their own history.

Slow-host load tests mock the native network boundary. A separate authenticated HTTP integration exercises two mobile clients against the real host and broker. These are not two-device Tailscale load tests. Chromium tests exercise the bundled terminal page, not Android WebView or iOS device gestures. No mobile emulator was attached during this verification. Native macOS execution and iOS compilation were not available. The Go race detector was attempted but could not run without a Windows C compiler.

Network recovery was verified against a real isolated tsnet control server, DERP and STUN fixture, not by disrupting the user's Wi-Fi. It recovered the same node identity, public key, IPs, preferences, listener and proxy URL without restarting the host or PTYs. Detection waits for SDK connectivity evidence before the recovery grace and backoff; it is not instantaneous. A real data-path failure can still reconnect a view or require Take after ownership moves.

Delivered approval requests last for one hour without a heartbeat. OS process death or lost connectivity can prevent a request from being delivered. Attempt UUIDs are ephemeral; a new process cannot safely cancel an unknown older attempt, which remains until host resolution or expiry.

The installed host and phone were not updated. Live checks were read-only; only isolated test applications and their terminals were stopped. Android test signing is not production release signing.
