# Prepared observer-v2 backend

The implementation uses the existing `shellfox_remote::Backend`, `ApprovalControl`, owner channel, `RemoteDispatcher`, `RemoteState` and Service. It creates no second Host, manager, PTY or parser.

## Entry points

- `Backend::prepare_observer_v2(self) -> Self` prepares an incarnation and bounded attempt/connection registry. `Backend::new` remains unprepared.
- `Backend::handle_observer_v2_signed(Endpoint, &[u8], SignedHeaders) -> ObserverHttpReply` verifies the original fixed POST path/body/transport identity and existing replay/rate limits. It returns only canonical `observer_v2::Reply` or `BoundReply` bytes. An unprepared backend returns HTTP 404 with no body.
- `RemoteBackend::observer_v2_signed(...)` is the validated signed-adapter entry point on an assembled facade. Path-only native factory constructors have been removed. The native bootstrap now retains a `RemoteHostSetup` envelope before borrowed initialization; its static approval-storage gate yields the typed local-only outcome before optional remote IO or binding.

These entry points do **not** admit V2 paths to the network listener. The listener still has the five unchanged V1 paths. No default client, UI toggle or source-sharing grant is enabled.

## Fixed approved-device policy

Approved devices receive server-fixed Write authority and one explicit `ApprovedFullAccount` file capability. The capability uses the actual host OS account and OS ACLs, not roots[], a guessed drive or root enumeration. Native one-click approval accepts only the peer ID and resolves actual authenticated pending state.

The private store retains exact historical V1/V2 readonly inventory and the closed `control-full-account-v1` policy. Native approval writes and runtime authority installation are now gated pending storage review, including valid V2/no-marker inventory. Existing legacy pending-marker presence/error remains denial; V1 normalization cannot install a grant without its gated save. No new marker/schema reader or durability protocol is approved. Accepted in-memory policy fixtures are separate, not persistence proof. Legacy fileRoots metadata does not determine authority.

This device policy does not loosen observer-v2 input restrictions or intrinsic raw/auth-pending states. The manager-v2 contract/backend is separately implemented but not default activated. Generic V1 control is evaluated by its actual adapter; limited formatted-active-buffer recovery is not FullState, and the native approval gate is not a claim that V1 control is universally unsupported.

## Owner dispatch

`OwnerCommand::ObserverV2(Box<ObserverOwnerRequest>)` carries the host-local `ObserverConnection` and Pair, Execute, Poll or Disconnect operation through the existing bounded owner channel. The connection wraps the single canonical Binding and existing Actor. Approval and current-connection fences serialize with owner commit.

The signed request handler checks both `decoded.check_connection(expected, actual_transport_peer)` and `command.authorize_profile()`. Existing per-method permissions still apply. The 43 V2 mappings are typed; terminal input, takeover, resize and terminal upload remain denied. CloseTab/DeleteSession use Service's original-source lifecycle cleanup, not an observer grant.

Pair attempts use a server-private approval client ID. Withdrawing one V2 attempt cannot deny an unrelated V1 client that happens to use the same wire client UUID. Replacement invalidates old tickets and retires their observer leases. Cancellation releases the operation/poll slot through Drop.

## Facts and compatibility

V2 catalog mapping walks authorized saved session metadata per entry. Only unauthorized elevated tab identities are excluded before tab facts, diagnostics and counts; an all-elevated session retains its saved header with tabs[] and zero authorized counts. Unknown creation facts, source inspection, resource measurements and timestamps stay explicit. It never calls the V1 whole-catalog mapper to fabricate V2 output. Capability collection uses the same immutable owner watch and native discovery worker through a capability-only entry point.

V1 whole inventory stays strict. Internal target scope maps only the response facts required by Rename/env/Close/targeted statistics. File/profile/home operations collect capabilities without unrelated session facts. The full captured snapshot and request fence remain unchanged; this is not a partial-catalog mode.

## Delivery and revocation

Attach requires the exact native eligibility epoch, actual sealed Normal PTY launch evidence, current metadata/peer permission and current connection/generation. It allocates the shared owner viewer ID and uses the authoritative Service observer-grant offer plus actual stamped recovery. Provisional books do not own Unobserve; the Service atomically transfers central custody and marks the validated book adopted. Raw/SSH/guest/generic program sources remain LocalOnly under the initial policy.

Wire recovery UUIDs map to exact opaque Runtime RecoveryIds. Recovery includes the actual same-barrier geometry and canonical layout revision. It is formatted-active-buffer-v1, not FullState. SourceEnded does not become process exit or cleanup proof.

Revocation clears active mailbox custody, queued bytes, layout and recovery tokens before bounded Unobserve admission. Full retains idempotent detach work for the same owner loop. Disconnected does not fabricate Closed or drop cleanup custody. Late targeted output cannot repopulate a revoked book. No source process is killed by observer detach.

## Gates still off

Network admission, native adapter/artifact acceptance and real owned-source end-to-end acceptance remain separate gates. The signed-handler tests use offline authentication vectors and injected owner transport. The actual native allow test is opt-in and requires an explicit standalone supervisor. It permits only an actual Normal capture; elevated/admin inheritance must deny, without token overrides.
