# TypeScript wire fixtures

These are JSON response bodies, not Rust-generated output. All field names retain the TypeScript wire casing. The manager snapshot and terminal examples are independent scenarios; their IDs do not refer to one another.

| File | Source and use |
| --- | --- |
| `manager-snapshot-success.json` | `success(snapshot())` from `src/renderer/test-fixtures.ts`, including its default session, settings and probe. Response to `getSnapshot`. |
| `terminal-attach-owned-success.json` | `success(attachment)` from `mobile/src/client/test-fixtures.ts`. Response to `attachTerminal`. |
| `terminal-attach-recovery-success.json` | Adapted from that mobile attachment: `truncated` is true and the chunk is the ANSI screen content used in `src/main/terminal/backend-restore.test.ts`. Illustrative schema-valid recovery, not a byte-for-byte captured serializer output. |
| `terminal-attach-blocked-success.json` | Mobile fixture IDs and epoch with the blocked shape asserted in `src/main/terminal/control.test.ts`. It contains no terminal content. |
| `terminal-events-success.json` | Mobile fixture IDs and epoch; the live data value comes from `src/main/remote/host.test.ts`. This is a `/v1/events` poll response. |
| `terminal-control-event-success.json` | Mobile fixture IDs, with an incremented epoch and blocked control event matching `src/main/terminal/control.ts`. This is a separate poll scenario. |
| `remote-methods.json` | All 43 method names from `src/shared/remote-schemas.ts`; checked against both Rust `REMOTE_METHODS` and the active TypeScript allowlist. |
| `remote-parity.json` | Schema fixtures for the remaining 16 file-transfer, image/asset, statistics, directory and terminal-upload methods. Values follow `file-transfer-schemas.ts`, `file-schemas.ts`, `process-stats-schemas.ts` and `terminal-upload-schemas.ts`; the redacted stats session comes from `manager-snapshot-success.json`. Validated by `tests/unit/rust-client-parity.test.ts`. These are protocol examples, not captured production responses. |
| `remote-discover-success.json` | Exact discovery response asserted in `src/main/remote/host.discovery.test.ts`. |
| `remote-pair-success.json` | Pair response in `mobile/src/client/test-fixtures.ts` and `src/main/remote/host.ts`. |
| `remote-auth-failed.json`, `remote-approval-required.json`, `remote-access-denied.json` | Exact host error bodies in `src/main/remote/host.ts`. |

The remote host accepts POST `/v1/discover`, `/v1/pair`, `/v1/request`, `/v1/events`, and `/v1/disconnect`. There is no hello endpoint. Native transport owns discovery and authentication; `mobile/src/client/protocol.ts` exposes the other four routes. Discovery returns protocol version 1. Pairing requires `terminalControl: "exclusive-v1"`; this is not the manager RPC `version: 1` envelope. Remote operations use `{ clientId, method, payload }`, not that manager envelope. Pair, events and disconnect use `{ clientId }`.

The host verifies HMAC peer headers before discovery or any other route. Discovery does not require approval. Disconnect occurs before the approval checks; other routes can return authorization or approval errors, and requests and polls also require a paired view. Permission errors are distinct from terminal control ownership. A blocked attachment is a successful response but grants no output or control.

For `snapshot: true`, chunks carry a serialized terminal screen as ANSI sequences at the specified size, not Ghostty serialized snapshots. They are distinct from ordinary raw PTY replay. The mobile test fixture itself uses the simpler string `hello`; the recovery example is an illustrative ANSI sequence and does not claim to be a captured xterm serialization. Sequence values and control epochs are JSON numbers. A fresh control epoch fences earlier writes and acknowledgements.
