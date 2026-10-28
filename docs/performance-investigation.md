# Startup and terminal retention investigation

Measured on Windows x64 on 2026-10-27. The intermittent minutes-long stall was not reproduced. These fixes address defects demonstrated separately; they do not establish a single cause for every reported hang.

## Shellfox

Closed terminal entries retained xterm parsers, scrollback cell buffers, PTY wrappers and event subscriptions until eviction or shutdown. Closing a terminal now freezes a bounded final screen and releases its parser. PTY resources are released only after authoritative exit, confirmed descendant cleanup and completion of close waiters. Failed cleanup retains ownership for retry. The existing 256-entry replay retention limit is unchanged.

An isolated probe created 24 terminals with real xterm mirrors and fake PTY transport, filled each with 3,100 lines, then exited them. After garbage collection, retained JS heap growth fell from 46.72 MiB to 21.84 MiB; ArrayBuffer growth fell from 66.91 MiB to 0.06 MiB. Both versions retained 24 replayable closed terminals and no live terminals. This measures retained resources, not a long-running native-process soak.

Process tracking also had three liveness problems:

- Repeated identical watches invalidated useful in-flight scans and started replacements. Unchanged watches now preserve scans and polling schedules.
- A Windows cached-birth scope above 30,000 characters prevented all later scans, including the scans needed to prune that cache. The scope now travels through stdin instead of an environment variable.
- Disposing a tracker could leave poll callers waiting for an uncooperative provider. Disposal now settles callers and suppresses late results.

Fresh Windows scans still cost about 0.8–0.9 seconds on this machine. They start PowerShell, compile the token reader and perform two CIM enumerations. These fixes remove wasted scans and permanent failures; they do not substantially reduce the cost of one scan.

The shared host changes apply to desktop and headless hosts and all their clients. Native macOS/Linux execution and mobile device acceptance were not performed in this investigation.

## Pi on this machine

Pi's core `--version` took 0.274 seconds and minimal help took 0.336 seconds. Normal help took 15.680 seconds. Extension timing identified two expensive imports:

- `oira666_pi-free-swarm` eagerly loaded its runtime even when swarm mode was disabled. The installed extension now imports that runtime on demand, preserving role selection, flags, commands and shutdown behavior. Its measured import fell from 6.669 seconds to 0.146 seconds.
- `oira666_pi-subagent` loaded a 129-module source graph. Its installed extension entry now points to a prebundle built from the same source, with Pi and TypeBox left external. No subagent source logic changed. The first measured bundled import took 0.561 seconds instead of 3.829 seconds.

A fresh ConPTY launch reached the final `session_start` observer in 12.015 seconds before the fixes and 0.984, 0.974 and 0.932 seconds afterwards. The after median was 0.974 seconds, about 92% lower. Normal help fell to 0.907 seconds. These are local startup measurements, not model-response latency or proof of fixing the unobserved minutes-long stall.

The local patches are under `C:/Users/user/.pi/agent/npm/node_modules/`. They change the swarm extension's `index.ts`, `src/session-commands.ts` and `src/extension-api.ts`, and the subagent package's extension entry plus generated `startup.js`. Package updates may overwrite them. Exact backups, patches, a prebundle build script, tests and a detailed report are in `tmp/pi-startup/`. No Pi core, settings or authentication files changed. Existing sessions were not interrupted; new Pi processes use the patched extensions.

Pi-side verification passed nine regression tests and a typecheck of the patched swarm graph. Actual Pi integration checks preserved inactive/enabled tool declarations and exercised the flag, environment and enable/disable command paths. Original and bundled subagent tool schemas, descriptions, commands and active-tool declarations matched.

The durable fixes were subsequently ported onto the fetched WSL repositories, committed, pushed and published as `oira666_pi-free-swarm@0.5.2` and `oira666_pi-subagent@0.7.7`. Their release tags are `v0.5.2` and `v0.7.7`. Both were tested against Pi 1.4.0, including packed-package checks. Downloaded npm artifacts matched the verified release tarballs by SHA-512. Normal package updates can now obtain these fixes without reapplying the local patches.

## Verification

- `pnpm typecheck` passed, including mobile.
- `pnpm test:unit` passed 3,191 tests across 237 files; 56 tests were skipped.
- `pnpm build:test` passed using the cached Go toolchain at `tmp/go-toolchain/go/bin/go.exe` via `SHELLFOX_GO`.
- Windows Electron checks passed for startup migration, real PowerShell Pi detection, and ten repeated second-tab Pi launches. The selected TUI-restore check was skipped by its platform requirements.
- New unit tests cover final-screen fidelity, delayed exit delivery, public-write fallback draining, cleanup failures, unchanged watches, large Windows birth caches and disposal cancellation.

Evidence and diagnostic tools are in `tmp/terminal-retention-*`, `tmp/tracking-*.log`, `tmp/perf-*.log` and `tmp/pi-startup/`. These scratch files are not part of the committed application.

The running installed Shellfox was not replaced or restarted, because doing so would terminate its active terminals. Source changes require a rebuilt installation and a safe restart.
