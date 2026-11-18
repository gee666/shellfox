# iOS observer-v2 source integration

This is an explicit delivery-only source route, not an enabled or accepted application backend. The default creator and old symbols remain v1. ABI extension 1 does not change wire v1 or the engine's ABI layouts. Observer wire version is 2.

The canonical contract is `crates/shellfox-protocol/observer-v2.md`. Native code uses the actual additive declarations in `crates/shellfox-mobile/include/shellfox_mobile_client.h`. It does not translate an observer attachment into v1 Owned/Blocked, invent creation facts/timestamps, or infer process exit from source EOF.

## Separate factory on the existing module

Carter's additive selectors on `NativeShellfoxRustClient` are implemented by the existing `ShellfoxRustClient`:

- `observerV2AbiVersion`
- `clientProfile`
- `createObserverV2Client`
- `discoverObserverV2`
- `pairObserverV2`
- `observerV2Operation`
- `pollObserverV2`
- `reconnectObserverV2`
- `disconnectObserverV2`

Common cancel, readEvent, bind/unbind and closeClient support the explicit observer creator. `closeClient` closes the client, with bounded withdrawal first. `closeTerminal` closes an engine on its native owner. An engine handle never enters `sfm_client_profile`.

`clientProfile` returns exactly `{profile, apiCapabilities}` from the native client handle. V1 is profile 1/capabilities 7. Observer is profile 2/capabilities 1. These are available API flags, not server permission, control or layout grants. Discovery, attempt identity, pairing echoes and actual bindings remain Rust checks.

V1 connect/reconnect/operation/poll entry points reject an observer handle. There is no automatic creator switch, hidden legacy pair or input path. A canonical legacyAvailable result is only an offer for a caller to consider separately.

## Status carrier and lifecycle

`SFGoTransport` retains the v1 `MobileRequest` string/error function. An observer uses `MobileRequestWithStatus` and immutable `MobileRequestResult` getters `statusCode` and `body`.

Those Objective-C names/types follow the pinned gomobile generator source. No generated Go headers or new binaries were produced for this source change. Fresh matching gomobile output is a build prerequisite; an old header lacking RequestWithStatus must not be patched or bypassed.

Every observer HTTP completion forwards the received 100..599 status to `sfm_observer_v2_complete_http`. Non-2xx bodies must be empty. Errors, timeouts, cancellation, missing results and malformed responses have status zero and no body, never an invented 404. Native code does not inspect English diagnostics or JSON business errors to choose fallback. Only Rust can recognize an actual 404 for the live pending DiscoverV2 request, before pair admission. 401/403, other endpoint failures, malformed data and network errors cannot become a legacy offer.

`SFNetworkPolicy.h` shares the exact ten carrier paths with the legacy bridge. No prefixes, query strings, encoded routes or trailing-slash variants are accepted. Only exact v1/v2 events paths use poll slots. Exact pair/events/disconnect requests have finite UIKit completion windows, ended on completion/deadline/expiration. This is not indefinite background execution. Logical cancellation fences the request but does not claim Go socket abort or release a physical slot early.

## Native consumption and fixed geometry

`SFClientProfile` supplies separate function tables for v1 and observer binding, unbinding, reset-ticket/grid/done, recovery-ready, consumed, polling and disconnect. Common dispatch/cancel/read_event/close remain common. The native record selects the table from the verified client profile, not the JSON event or a JS readonly flag.

A bound observer sets `deliveryOnly` in the owner and frame mailbox. Engine input and queued write/resize admission fail before encoding/sending. UIKit cannot enable editing by passing readOnly=false. Local pan/selection/copy and validated user-confirmed HTTP(S) links remain passive native actions.

The owner obtains the current reset ticket and its actual canonical host grid, resets at that grid and completes the matching handshake. It pumps recovery through Ghostty before calling the observer consumed API. Recovery ACK and data ACK are prepared by Rust from the consumed native ticket, not the sequence argument from a caller or receipt/enqueue time. Mirror query replies and clipboard effects remain discarded.

Observer viewport/font changes save local presentation bounds only. They never resize or reflow the canonical parser, including after recovery becomes ready. UIKit clips/pads the fixed grid. There is no PTY resize, ResizeExternal or layout grant. A canonical layout event requires a new stamped recovery through the Rust cursor rules.

Observer events remain profile-tagged canonical models. Connection state contains the actual binding or null, without v1 permission/control fields. Coalescing uses that binding, status/error changes and reset tickets. Internal success-only completions and delivery byte streams stay native. Explicit requested poll completions are still forwarded even when tagged internal.

## Source checks and gates

`node --test mobile/ios/scripts/contracts.test.mjs` checks the actual RN-generated selectors/provider mappings, header symbol references, exact carrier policy, role guards and canonical fixture bytes. Final source verification passed all 11 checks, plus all eight shared `scripts/mobile-native.test.mjs` classifier/build-argument checks. Root/mobile typecheck passed. The scoped Rust-client TypeScript suite passed 46 tests with one opt-in C ABI capture test skipped in this invocation; the coordinating owner's separate joint-proof result is not relabeled as a local run. These are source checks, not an iOS compilation or native runtime result.

The test spec additionally contains source fixtures for:

- actual discovery 404 offering validated legacy discovery, without pair;
- 401 and error text containing 404 never triggering fallback;
- profile flags and observer input/layout denial despite a writable UI hint;
- canonical recovery grid retained across a smaller local viewport;
- no recovery/data ACK before native parsing, then distinct stamped recovery/data ACKs;
- no replay/data base64 in JS-facing coarse events and no sourceEnded-to-process-exit conversion.

The fixture JSON copies are byte-identical to the canonical protocol goldens. They are synthetic examples, not accounts, credentials or production captures. The coordinating Rust owner confirmed the final C ABI source checkpoint with 86 tests, Clippy and an actual C-ABI-to-TypeScript joint proof. That upstream evidence does not compile this Objective-C++ bridge. Native iOS execution remains blocked by absent Xcode and matching fresh iOS archives/gomobile bindings. The C++ ABI signature source was extended, but no new compiled iOS observer artifact is claimed here.

This source was reconciled with Lakshmana's final header/functions/events and Carter's actual additive Spec. Before any native iOS runtime check, require matching fresh Rust archives and gomobile bindings and an Xcode build. Do not relabel the old staged v1 artifacts. Four-ABI Android results, API35/4KiB fixtures and earlier v1 evidence do not establish observer-v2 or iOS acceptance. Preserve the both-platform adoption gate and explicit transition path.

No network, GUI, account, credential, global-process, install, signing or release action is part of this source pass.
