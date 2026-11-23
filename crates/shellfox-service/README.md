# Rust headless host increment

`shellfox-service` runs the experimental metadata host and owned terminal runtimes without GPUI or a display. The `shellfox-cli` binary starts it with `host serve`. Other CLI invocations connect to that same process through the shared typed local client. They do not open SQLite or start their own PTY.

This is not full Shellfox parity. Private metadata/settings use `private.local.v1`; six explicit local automation routes and `private.desktop.launch.v1` are separately typed. They do not widen the remote 43-method allowlist or mobile API. Native SSH and authenticated remote adapters share this owner when their trusted resources are installed, but their integration and platform acceptance remain separate from a completed migration.

## Run

Create a new empty directory under this worktree's `tmp/`. Pass its absolute path. Existing directories may contain only this experiment's artifacts; the host refuses unrelated files before changing permissions.

```text
shellfox-cli --data-dir ABSOLUTE_EMPTY_DIRECTORY --helper ABSOLUTE_RETAINED_HELPER --helper-sha256 TRUSTED_64_HEX_DIGEST host serve
```

The expected helper digest comes from the trusted build/package record, not a hash computed from an arbitrary executable selected by a caller. The helper is checked before each invocation. The CLI README lists the session, tab, settings and attach commands. `host stop` waits for owned process cleanup and returns a typed reply before the endpoint disappears.

## Ownership and security

- There is no TCP listener or TCP fallback. Windows uses a local named pipe with the exact current-user-only protected DACL proven by the retained helper. The first pipe instance is exclusive, remote clients are rejected, and both ends verify the peer user token. Unix uses a private socket and verifies the peer UID.
- The retained helper owns the existing `.host-lock/host.lock` kernel lock until its stdin closes. Loss of that helper stops further metadata requests. The Rust host also owns its experiment store lock.
- The endpoint directory and credential file require permission proof. Windows requires the retained native helper. Missing helpers, wrong artifact hashes, unsafe paths and invalid proofs fail closed before opening metadata. Unix permission checks use the transport crate's no-follow, owner/mode/link checks. Unsupported PTY ownership remains a runtime error, never an insecure launch fallback.
- The endpoint stores a random capability, never printed or placed in process arguments. The pipe carries a bounded authenticated frame. Request and response JSON are limited to 1 MiB and 8 MiB. Request deadlines and cancellation apply during transport reads and writes. No terminal output is persisted by this crate.
- Each live tab has one registered generation and one runtime owner. Each generation has a distinct private `terminal-home/TAB_UUID/GENERATION_UUID` home/config directory. Interactive native/local launches explicitly opt into installed executable paths; the PTY default remains Minimal, and no account auth/config is imported. Registration precedes startup. Closing a tab/session prepares metadata cleanup, waits for the exact runtime to reach `Closed`, unregisters that generation, then commits. Failed cleanup retains the metadata and owner for retry.
- Each private client has one authenticated lifetime connection and a server-created epoch, separate from per-RPC connections. Explicit disconnect, drop or actual process death releases only that controller's raw stdin/pipe and input leases, not its processes. A replacement epoch allocates a fresh controller and cannot adopt old raw pipes. Old EOF cannot revoke the replacement. Lifetimes are bounded at128 separately from16 request tasks; no automatic reconnect or input replay.
- Output stays in bounded memory until the client acknowledges consumption. Recovery carries a sequence barrier, and stale generations/epochs cannot acknowledge or write. Recovery currently restores only visible cells. It does not restore TUI modes, inactive buffers, saved cursors or scrollback.

The strict private lifetime frames and response-consumption handshake are described in `docs/private-client-lifetime.md`. Endpoint transport versions fail closed instead of falling back to another owner.

One dedicated owner thread holds the metadata host and runtime map. A bounded command queue and asynchronous replies leave the permission/ownership supervisor and transport deadlines runnable during PTY waits. Dropping a request does not cancel or retry a mutation with an uncertain outcome. A random response trailer must be consumed and acknowledged before normal stop tears down the endpoint.

The owner loop has a bounded-dispatch hook for separately authenticated remote adapters. Such adapters must recheck their approval epoch on that same thread before mutation. They must not construct another host or terminal map.

## Native views and settings

Native desktop clients use the same owner queue and runtime map. A cloned client shares a view identity; `fork()` allocates a new UUID for another window. The owner maps those UUIDs to controller IDs. Native actions validate before enqueue, including a 1 MiB aggregate metadata/settings bound and 64 KiB key/paste bound. Typed `Full` returns the unsent action and is the only automatic retry case. A reply lost after enqueue has an uncertain outcome and is not converted into `Full`.

The sole runtime event consumer records native control/observer events, then captures one latest immutable screen per tab. Frame IDs change on selection or resize even when the output sequence does not. Offering a frame grants no output credit. A bounded client accepts it before acknowledging its delivered sequence or recovery barrier. Read-only observers use their own leases and never claim terminal control.

The owner installs the settings runtime adapter with its final ownership-loss token before publishing readiness. Available Windows local profile changes use current shared discovery, canonicalize the selected executable, and reject arbitrary executable overrides. An unavailable opaque saved profile remains metadata during unrelated saves. Linux/WSL changes that require Python validation remain gated until an owned probe provider is installed. This is not full shell-settings parity on those targets.

## Raw automation and native launch

Raw automation retains one original process owner and one passive canonical parser. The parser starts before raw launch with an immutable reserved identity and `LegacyAutomationV1` display policy. Raw stdout/stderr remain byte-exact; visual decoding does not create another PTY, input authority or upstream query responder. Stdin starts only after containment and READY. Actual OS-write receipts are deferred while the same owner and bounded concurrent listener keep output draining.

Show publishes those reserved IDs with durable receipt/collision checks and unknown shell creation facts; it does not reconstruct a parser from a tail or implicitly select/focus all windows. The existing `automation-pipe` marker selects read-only UI, not authority. Source finish retains the parser for late Observe/Recover. Explicit metadata close stops/drains the raw owner and joins the original parser before commit; opaque cleanup proof survives metadata failure and retry. Passive observer detach does not stop raw execution.

A failed source is different from successful final rendering. Original raw cleanup and original parser join must both be proved before a separate typed presentation warning is returned. Bounded opaque failed bytes/evidence are retained in memory, never Debug/serialized/logged. `DirtyAfterOwnershipLoss` remains an independent activity-persistence warning; ordinary persistence failure retains Service/touches rather than replaying input.

Native desktop registration/claim/ACK remain non-serialized in-process capabilities. Private Discover/Submit/Receipt handoff targets the one registered coordinator with request-ID deduplication and owner fencing. Owner-without-desktop cannot become a renderer through this protocol: headless-to-rendering attachment remains an explicit migration blocker, not permission to start another Host.

## Activity baseline

Electron intentionally uses `sessions.updatedAt` as persisted last activity. All authoritative live PTY data and final exit refresh pending TTL, including idle escapes and input echo. The meaningful-output filter controls BUSY only. Replay, rendering and polling do not produce live activity. Pending touches flush once per minute or during shutdown, as in `src/main/session-lifecycle.ts:32-49` and `src/main/repository.ts:114-115`.

Raw external-source activity integration must likewise use the shared authoritative sink after metadata publication; native Show by itself is not evidence of complete activity or remote/mobile parity.

An earlier proposal to let only meaningful output refresh TTL was withdrawn. It was not an approved behavior change. No separate persisted last-activity field is intended.

## Checks

```text
cargo test --locked -p shellfox-service --lib
cargo clippy --locked -p shellfox-service --all-targets --no-deps -- -D warnings
```

The ignored private-server integration test requires `SHELLFOX_SERVICE_TEST_HELPER` and `SHELLFOX_SERVICE_TEST_HELPER_SHA256`, pointing to a retained helper artifact and its trusted digest. It uses a fresh directory under project `tmp/`, verifies bad credentials and ownership contention, exercises two typed clients, and stops only its own server. The CLI crate also contains an owned Windows PTY integration test. Neither test uses installed application data or discovers/kills unrelated sessions.

Windows x64 is the current native verification platform. Other supported targets still require native acceptance. A compiled conditional Unix backend does not establish macOS/Linux runtime parity.
