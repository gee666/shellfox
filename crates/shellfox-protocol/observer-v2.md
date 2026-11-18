# Observer-v2 implemented contract checkpoint

This is the canonical protocol/client implementation checkpoint, not an enabled production route. Go, backend and mobile/native adapters must agree on these fixtures before enabling it. Existing v1 modules, strict shapes, 43 verb strings and byte fixtures are not reinterpreted. The default `shellfox_client::Client` remains v1.

## Source and fixtures

- Protocol exports: `shellfox_protocol::observer_v2`.
- Client exports: `shellfox_client::observer_v2`.
- Golden JSON: `crates/shellfox-protocol/tests/fixtures/observer_v2/`.
- `contract.json` records exact paths, selections, 43 methods, profile-denied methods and limits.
- `methods.json` contains a typed request and response example for every verb, including explicit profile failures.
- `handshake.rs`, `replies.rs`, `catalog.rs`, `session.rs`, `tab.rs`, `delivery.rs`, `requests.rs` and `command.rs` are the actual closed DTO/schema definitions. There is no arbitrary JSON command path.

The fixtures are protocol examples, not production captures. Their README identifies the old TypeScript-derived normal session and illustrative recovery payload. The complete snapshot deliberately retains a raw source with unknown creation facts and an unavailable inventory entry.

## Fixed carrier and selection

All requests are POST. Additive Endpoint variants map only to:

| Variant | Exact path |
| --- | --- |
| DiscoverV2 | `/v2/discover` |
| PairV2 | `/v2/pair` |
| RequestV2 | `/v2/request` |
| EventsV2 | `/v2/events` |
| DisconnectV2 | `/v2/disconnect` |

No arbitrary route, URL, address, port, method or headers are accepted by this API. Existing v1 variants retain their old paths. This code does not enable a Go/backend route or change a mobile C ABI.

Profile is exactly `observer-v2`, version exactly 2, catalog mode exactly `complete`, delivery format exactly `vt-bytes-base64-v1` and recovery format exactly `formatted-active-buffer-v1`. Discovery advertisements may contain up to 16 unique bounded identifiers per list, each 1..64 ASCII alphanumeric/hyphen bytes. Unknown advertised identifiers can participate in safe intersection; a selected identifier is always the exact typed supported value.

Discovery request is `{}`. `Reply<Discovery>` returns version/ok/value or version/ok/error. PairOffer includes version, clientId, attemptId, ownerId and the exact selected strings. PairSelection echoes client/attempt/owner/selections and adds connectionId. The client rejects any mismatched echo. A changed offer under the same attempt ID must be rejected by the server's attempt registry.

## Binding and actual authentication

Binding contains clientId, ownerId, connectionId and profile. Id is a strict non-nil RFC UUID type. None of these identifiers alone proves authentication or authority.

The backend constructs ConnectionBinding from its actual verified incoming peer context and the server-issued selection. `DecodedRequest::check_connection` compares that actual peer and every binding field. The transport peer is not accepted from JSON. Approval and actual owner lifetime remain server checks.

`decode_request(original_bytes)` first inspects only routing with IgnoredAny, then deserializes the original bytes into the exact selected Envelope<Request>. It does not erase duplicate payload fields through an intermediate Value. Native registration, claim, completion and local automation methods are absent from the 43-verb table.

BoundReply includes version, exact binding, ok and exactly one of value/error. Unknown fields, wrong versions, cross-branch nulls and mismatched bindings are rejected. Error has code, bounded message, retryable and a closed v2-only detail union. V1 error shapes do not change.

## Separate typed method mapping

V2 Request is sealed. Non-delivery payload wrappers reuse the existing strict field validators where semantics are unchanged, but V2 response typing is independent:

- GetSnapshot returns Snapshot with complete coverage and per-entry facts.
- Session-returning methods return CatalogEntry, including explicit unavailable entries.
- GetSessionStats returns V2 SessionStats with redacted known environment arrays.
- AttachTerminal, AcknowledgeTerminal and DetachTerminal have the new delivery-only DTOs.
- SetTerminalEnv returns the presence-aware V2 Settings leaf, not legacy SettingsDto.
- Other responses retain their established value schemas inside BoundReply only where they do not insert migration defaults or erase optional presence.

`Command::authorize_profile` must be checked by the backend in addition to connection and operation permission. The initial implementation rejects takeover, terminal input, terminal resize and the four terminal-upload verbs under this profile. CloseTab and DeleteSession remain admissible manager lifecycle requests, subject to independent backend permissions and original-source cleanup. They must stop/drain the actual source through Service lifecycle before runtime removal, not route through observer input/layout or generic unfinished-runtime close. A valid payload does not grant permission. Allowed manager/file requests still require actual backend permissions and source lifecycle proof. Delivery grants no cleanup authority, and source-kind metadata cannot substitute for that proof.

The explicit client enforces profile denial before transport. `execute_command` uses one exhaustive typed match and serializes a presentation value only after the typed response has passed validation. It is not an untyped bypass.

## Catalog and facts

Fact is the closed union Known(value), Unknown(reason), NotApplicable. Unknown reasons are notRecorded, pending, unavailable and invalidEvidence. Known absence uses a nullable Known value; a missing Known value is invalid.

Session/Tab facts are explicit closed named fields, not an arbitrary property map. Actual IDs and titles remain separate. SourceKind is a fact about pty, automationPipe or externalBytes registration, never an input, layout or cleanup capability. Missing creation adapter, shell, profile or timestamp facts remain unknown/not-applicable as proved, not backfilled from settings, current time or another tab.

CatalogEntry is Session with a boxed Session value, or Unavailable with actual sessionId and bounded issues. Boxing does not change JSON. Snapshot validates complete mode, entryCount, unique entry IDs, tab membership/uniqueness and all known leaf bounds. It cannot prove inventory completeness by itself: the backend must enumerate every authorised entry and apply visibility policy before diagnostics. Partial mode is not representable.

Unrelated unprojectable entries do not invalidate a known normal entry. Individually projectable operations must remain independent. A strict v1 request requiring unsupported full coverage must return its existing typed UNSUPPORTED failure, not a filtered success, placeholder facts or fake Owned attachment.

## Settings presence and reused-leaf audit

`Snapshot.settings` is `Fact<observer_v2::Settings>` and SetTerminalEnv returns that same V2 leaf. It never deserializes through v1 SettingsDto. There is deliberately no From<V1SettingsDto> conversion: after legacy migration defaults have run, the original omitted-vs-null/value provenance is already lost.

PythonPath and terminalProfileId retain three states: omitted, explicit null and a present value. BackgroundColor, historyPageSize, sessionSettleHours, sessionDeleteHours, autoUpdate and terminalEnv are optional but non-null; omissions stay omitted rather than becoming background colors, durations, booleans or empty arrays. Required nullable shellExecutable still requires an actual field. Backends must construct present values or known absence from actual evidence, never infer them from a legacy DTO default.

V2 ProcessRule also keeps optional processNames absent and rejects explicit null instead of coercing it to omission. The audit of reused probe/capability, terminal-profile, file, changed-event and statistics leaves found only omission-preserving optionals or required nullable fields, not migration-value defaults. V1 SessionDto.env defaulting is not used by the V2 session facts model.

The 43-method golden roundtrip test compares every object key and null without changing either side. Only numeric lexical representation such as 24 versus 24.0 is compared semantically. Golden bodies are not edited to accommodate inserted Python paths, timestamps or defaults.

## Delivery, recovery and geometry

Attachment is either Attached with DeliveryRef, required input:null, FixedCanonicalGrid layout and Recovery, or Unavailable with a bounded reason. It is never a v1 Owned/Blocked conversion. DeliveryRef binds tab, generation, deliveryId and positive safe deliveryEpoch.

Recovery carries recoveryId, exact format/encoding, canonical base64, lastSequence, actual cols/rows and canonical layoutRevision. Decoded recovery is at most 4 MiB and begins with the formatter's CAN + RIS prefix. It is not FullState. Refuse unavailable continuation/formatting or excess bounds; do not truncate or relabel VisibleScreen.

Backend recovery fields must come from ONE actual stamped Runtime capture. lastSequence means last delivery sequence, not upstream raw source sequence. The backend retains the recoveryId-to-lease/generation/boundary/geometry/revision association. A recovery ACK maps to Runtime layout acceptance only after validating that exact association. No standalone wire layout ACK or layout grant exists.

Data uses canonical base64 and delivery sequence, at most 16,384 decoded bytes. SourceEnded is distinct from ProcessExited. ProcessExited requires an independently evidenced outcome; source EOF, source kind, display state and exit fields are not process cleanup proof.

A layout event requires recovery. The client stops consuming/ACKing later data and requests a new attachment/recovery. It never calls ResizeExternal or PTY resize. The server must order layout boundaries, retain a bounded journal and recover on gaps, or retire the old lease and issue a new recovery. layoutRevision must be the actual canonical grid revision, not a frame ID or byte counter.

## Client and explicit ACKs

Construct ObserverClient explicitly with its transport destination and v2 client Id. Discovery takes FallbackPolicy::Never or OfferLegacy. It spawns no background tasks and never retries a mutation. The client permits eight operations and one poll, separately from Go's shared 12-operation/64-poll limits.

DeliveryCursor stores the current Binding and actual DeliveryRef. All calls require the current client binding. Receiving an event changes received sequence only. After the native consumer applies the exact recovery/grid, consumed_recovery returns a typed recovery ACK for its fixed boundary. After consuming all bytes through a received sequence, consumed_data returns a data ACK. Neither sends anything automatically.

validate_ack rejects fabricated, future, stale, pre-recovery and wrong-stamp requests. Repeated recovery ACK preparation cannot rewind the consumed cursor. Gaps and canonical layout changes invalidate old recovery/data ACKs until a fresh attachment is consumed. Source end forbids later data but does not infer original-process exit. Revocation forbids credit; same-binding detach remains harmless cleanup.

The backend still validates every ACK against its actual offered boundary and observer lease. Client bookkeeping is not server authority. Native bindings must use the real native consumption boundary, not caller-supplied JSON assertions. Attach/recovery, poll data and ACKs require native engine-binding handling; a mobile adapter must not forward the generic execute_command recovery value or high-volume event data to JavaScript. Passive observer mirrors must discard query replies and clipboard effects.

## Fallback, cancellation and withdrawal

Only before any v2 pair admission: an actual TransportError::Rejected{status:404} from V2Discover, or a valid incompatible discovery/profile response, may lead to validation of the unchanged v1 discovery response. Generic business UNSUPPORTED, auth/approval failures, malformed responses, timeouts, cancellations and network failures cannot trigger fallback.

LegacyAvailable is a visible offer only. It never pairs, attaches or acquires control through v1. Pair admission permanently closes automatic fallback for that client instance, including uncertain outcomes and reconnect. A caller may explicitly choose a separate legacy client; the existing default client is unchanged.

Dropping a future cancels its child transport token and releases matching-cycle concurrency state. Same-cycle local cancellation preserves the connected binding and unrelated requests. Cycle loss returns stale. Transport deadlines, bad responses and revoked approval invalidate the binding without replay. Pairing and ordinary requests have no hidden retry loop.

Disconnect has an exact attempt variant for late/pending pairing and a connection variant for a paired view. It withdraws only that authenticated peer's attempt/subscription. No Stop or raw-process cleanup is implied. Reconnect invalidates old local state; callers should withdraw a view before replacing it when possible.

## Limits and acceptance boundary

Encoded request 1 MiB; response 8 MiB; native deadline 30 seconds; server poll 20 seconds. Polls contain at most 1,024 events and 256 KiB decoded terminal data, additionally bounded by encoded response bytes. Recovery/data base64 has canonical padding and zero unused bits. Sequences/epochs use safe integer bounds; partial geometry/facts are never invented to fit limits.

Tests use injected transports and golden examples, not production routes, accounts, network or native processes. New client/protocol tests do not establish Go status propagation, backend source/permission correctness, passive mobile consumption or four-ABI acceptance. Those are coordinated integration gates. Old staged v1 artifacts remain immutable and must not be relabeled as this implementation.
