# Shared outbound client

`shellfox-client` contains no local host, PTY, parser, filesystem, native helper or GPUI dependency. Android, iOS, desktop and headless callers inject a `shellfox_protocol::OutboundTransport`. Native transport authenticates and routes the peer. A validated `AuthenticatedPeerId` is a destination string, not proof of identity or approval.

## Remote API

```rust,ignore
use shellfox_client::{Client, ViewCursor, protocol::*};
let client = Client::new(peer, fresh_client_id, transport);
match client.pair().await? {
    Reply::Success(_) => {}
    Reply::Failure(error) => return Err(error.into()),
}
let snapshot = client.execute(&GetSnapshot {}).await?;
let events = client.poll().await?;
```

`Request` is sealed to the existing TypeScript remote allowlist. `RemoteCommand` represents the same 43 methods. Presentation bindings can call `Client::execute_command(&command)`; its single typed dispatch validates each response before serializing the presentation value. Each request has a typed `Response`, `Validate` implementation and exact camelCase method name. All 43 methods in the active TypeScript remote allowlist are represented, including chunked file transfers, preview assets, image writes, directory completion, session statistics and terminal uploads. The request envelope is `{clientId, method, payload}`, not the manager IPC `{version, method, payload}` envelope. Protocol version remains 1.

Pairing requires `paired: true` and `terminalControl: "exclusive-v1"`. Approval-required, access-denied and authorization failures remain structured host replies. Transport errors are separate. `ClientState` reports connection status, permission, a successful-pairing counter and suggested retry delay. The caller drives pairing and polling; this crate spawns no background loop. Approval retries retain the client UUID. Reconnect fences in-flight work and requires a fresh cryptographic UUID from the caller. When possible, call `disconnect()` before replacing a view. Disconnect only withdraws the view and never kills host terminals.

There is no automatic retry, including mutations that fail with an uncertain transport outcome. A same-cycle local `Cancelled` result from an operation or poll releases only that intent; it does not change the connection to reconnecting or cancel unrelated work. A cancellation after the connection cycle changes remains stale. A dropped pair, poll or request future cancels its native call and releases its concurrency permit. Connection failures cancel the previous cycle. No more than eight operations and one poll run per view. The transport must enforce cancellation, the 30-second deadline, and encoded-body caps of 1 MiB request and 8 MiB response. The client checks the response cap again before decoding.

## Terminal consumption

Build a `ViewCursor` from a successful attachment and `ClientState.connection_id`. Discard cursors whenever that connection ID changes. Pass terminal events through `observe`. It fences stale generations/epochs, drops duplicate data and requests reattachment on gaps or a control transition back to owned. Blocked attachments grant no output or input capability.

Only call `cursor.consumed(sequence, connection_id)` after the view/parser consumes all bytes through that sequence. This returns a typed `AcknowledgeTerminal` request; send it explicitly with `execute`. Neither `poll` nor `observe` sends ACKs. `recovery_request` resumes from the consumed sequence, not the received sequence. Cursor-built input and resize requests require ownership.

The current v1 contract implies ANSI recovery. With `snapshot: true`, chunks contain an ANSI screen restore at the supplied dimensions. Otherwise they contain raw PTY replay. This is not a Ghostty binary snapshot. No binary recovery format or unsupported terminal capability is advertised.

## Input tokens and byte compatibility

`ViewCursor` retains the attachment's tri-state `settledAt`: absent, explicit null or timestamp. Matching-generation settlement events update it. `write()` clones the token into the returned request, so bindings must call it at FIFO admission rather than rebuilding input at dequeue. When an attachment request is in flight, bindings must preserve and reapply a newer matching-generation settlement event after applying the older reply. These tokens describe observed state; they do not grant control authority.

Rust `WriteTerminal` permits NUL as terminal input, including Ctrl+Space. Paths, environment values and file text retain their NUL restrictions. The authorized terminal-only compatibility fix also removes inherited TypeScript/input/helper NUL rejection. Focused schema, renderer, bridge, real xterm and owned raw-ConPTY checks are recorded in `docs/terminal-nul-input.md` at the workspace root. Native platform acceptance and rebuilt helper artifacts remain separate checks.

The existing v1 contract has no binary-input flag. Active desktop and mobile xterm code subscribes to `onData`, not `onBinary`; xterm sends DEFAULT/X10 mouse reports to `onBinary`. Host input is a UTF-8 string, including `Wire.Utf8.GetBytes(text)` in the elevated helper. Raw high-byte mouse reports therefore have no existing lossless route. Do not reinterpret Ghostty bytes as Unicode or invent a compatibility flag. Arbitrary byte input needs an explicit negotiated protocol extension with end-to-end host/helper tests, or a supported UTF-8-safe encoding constraint. No binary method was added here.

## Streaming file and upload lifecycles

All file and asset operations use typed POST requests on the existing `/v1/request` endpoint. Asset paths are host file references, not client URLs. There is no separate invented asset route or HTTP GET.

`FileReadCursor` validates bounded chunks against the declared size, offset and EOF. It advances only when the caller reports sink consumption. `FileWriteCursor` permits one mutation at a time, checks exact offset acknowledgements and refuses to replay uncertain writes. Both stream 192 KiB chunks without whole-file allocation and expose explicit transfer cleanup requests. Empty files still follow the begin/finish or final-empty-read lifecycle.

`ViewCursor::begin_upload` and `UploadCursor` bind terminal uploads to the controlling view, generation, epoch and connection. Takeover or reconnect fences further writes. Cleanup retains the original identity so it can cancel an abandoned upload after takeover. A positive finish acknowledgement wins over cancellation racing an already-issued finish. None of these cursors sends requests, writes terminal input or opens a local file.

Native bindings accepting presentation commands must call `ViewCursor::validate_upload_begin` before begin, then keep the resulting `UploadCursor` in native-owned state. Its `accept_write`, `accept_finish` and `accept_cancel` methods compare raw command fields with that lease before changing state. A caller-supplied generation, epoch or upload ID is never proof of native ownership.

Canonical base64 checks include padding placement and zero unused bits. The exact old-host upload rejection is translated to `UNSUPPORTED` without disconnecting an otherwise healthy view. Genuine authentication failures still invalidate the connection. No operation is retried automatically.

## DTO compatibility and limits

`shellfox-protocol::ManagerSnapshot`, `SessionDto` and `SettingsDto` describe the full active TypeScript wire shape. They are not aliases for `shellfox_core::Snapshot`, metadata sessions or preferences. The protocol reuses compatible core IDs and environment entries only. There is deliberately no automatic adapter that invents status, settings, capabilities, probe results or local runtime state. A host must provide real evidence for those fields or return `UNSUPPORTED`.

Response decoding rejects unknown fields, invalid result branches, unsafe numeric sequences, invalid control epochs and bounded terminal/file payloads. Optional nullable fields preserve omission separately from explicit null. Host-side authorization, elevated-profile exclusion, path policy and mutation commitment checks remain mandatory; client validation cannot replace them.

`saveSettings`, host stop, local shell creation, clipboard, OS integration and native-helper operations are not remote client methods. Existing remotely allowed environment updates use `SetTerminalEnv` and `SetSessionEnv`. No manager-only method can be selected through `RemoteCommand`. A complete client contract does not make a partially implemented host compatible: host adapters still need real file, statistics, directory and terminal backend implementations. Do not replace those backends with metadata-only successes or permanent unsupported replies.

## Private local experiment

The opt-in `local-host` feature exports separate `LocalRequest`, `LocalTransport` and `LocalClient<T>`. Defaults used by mobile exclude them. `LocalRequest` is unsealed so the service can define its own typed local request and response. `LocalClient::execute` accepts `private.local.v1`, the exact shared `DESKTOP_LAUNCH_NAMESPACE` value `private.desktop.launch.v1`, and the six typed local automation methods: `automation.start`, `automation.list`, `automation.read`, `automation.write`, `automation.stop` and `automation.show`. This is an exact allowlist, not an `automation.*` or `private.*` prefix rule. The client uses the protocol's typed automation requests and responses and retains the same byte/deadline/cancel limits. None of these methods can be selected through the sealed remote `Request` or its 43-method command enum. `LocalTransport` accepts an owned frame body and limits, with no remote peer or endpoint. It must verify private endpoint ownership and authentication before opening the connection.

Desktop launch request DTOs remain service-owned and contain only Discover, Submit and Receipt. The shared namespace admission does not validate their payloads or expose native registration, claim or ACK APIs. The existing local server owns routing and payload validation; the client adds no competing router, transport or global focus operation.

This local experiment is not a mobile-compatible v1 server. Its core snapshot/preferences DTOs remain explicitly separate from `ManagerSnapshot` and `SettingsDto`. The CLI must use the typed client and verified service endpoint, not open SQLite or launch a second PTY.

## Checks

```sh
cargo test --locked -p shellfox-client -p shellfox-protocol --all-features
cargo clippy --locked -p shellfox-client -p shellfox-protocol --all-targets --all-features -- -D warnings
```

Tests use an injected fake host, existing TypeScript JSON fixtures and explicit consumption bookkeeping. `cargo test --locked -p shellfox-client --no-default-features` also runs default-feature rejection tests and compile-fail examples proving that `LocalClient`, `LocalRequest`, automation contracts and the desktop launch namespace constant are unavailable without `local-host`. `tests/unit/rust-client-parity.test.ts` validates the remaining 16 method fixtures against active Zod request/response schemas and checks the complete 43-method allowlist. They do not start a network service or access production state. Fixture sources and the illustrative ANSI recovery sample are documented in `tests/fixtures/README.md`. These tests do not establish native mobile, real-host or platform acceptance.
