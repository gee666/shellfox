# manager-v2 backend

The backend consumes the compiled `shellfox_protocol::manager_v2` contract. The earlier JSON proposal is historical, not another supported envelope.

## Wiring and activation

`Backend::prepare_manager_v2` prepares canonical signed handlers over the existing owner-channel boundary; `manager_v2_signed` accepts a closed endpoint enum and the original signed body, not arbitrary URLs or headers. Native path-only factory constructors have been removed. Native bootstrap retains `RemoteHostSetup::retained` before borrowed initialization; the known static approval-storage gate produces `RemoteSetupOutcome::LocalOnly` before optional remote IO/binding/authority. All native profiles remain gated.

The default backend remains unprepared. The real HTTP allowlist remains V1-only. Preparing a backend does not activate source sharing, networking or default interactive migration.

Saved metadata, catalog, lifecycle, settings and file operations reuse `remote_v2_metadata.rs`. Only their unchanged typed operation payloads are shared; manager bindings are never converted into observer bindings. Approved devices must have Write and the explicit ApprovedFullAccount file capability. Device approval does not create terminal control.

## Delivery and control

Each attachment has one actual Runtime role: controller **or** observer. It does not install duplicate subscriptions. A controller's separate DeliveryRef, ControlRef and LayoutRef map to the same actual lease. An observer has no control or layout authority.

The existing Service drain is the only Runtime consumer. Native event capture borrows first; observer and manager mailbox taps move their targeted chunks and return unmatched events to the same drain. Each manager mailbox retains at most 128 KiB/256 chunks; the mailbox registry and notices are bounded. Offered bytes remain held until an actual corresponding ACK succeeds.

Recovery uses the actual `StampedRecovery`, including its opaque Runtime RecoveryId, geometry, canonical layout revision and sequence. The wire recovery UUID maps to that exact stamp. Controller ACKs and observer ACKs use their respective Runtime APIs. A layout change invalidates old recovery tokens and blocks input until full recovery is consumed. LayoutRef is an authority epoch, not geometry.

Takeover purges previous manager controller custody before bounded claim admission. Revocation removes queued bytes, recovery tokens and input/layout references before cleanup admission. Queue Full retains cleanup work; disconnect is not fabricated as source closure. Retiring a view releases control or unobserves; it does not stop the original process.

Native eligibility still requires the captured factory policy and actual trusted normal launch evidence. Unknown/elevated launches, raw sources and unapproved SSH/guest purpose expansions do not acquire control merely from a profile name or metadata marker. The elevated-policy boundary remains on hold.

## Input and receipts

Key input converts the canonical KeyEventV2 into the compiled Runtime OwnedKeyEventV2 with checked physical IDs, actions and modifier masks. Text, scalar and composition fields are preserved. There is no Character-to-Unidentified fallback, modifier-side inference or handwritten escape encoder. Mouse input uses the accepted recovery's actual opaque geometry stamp. Binary input remains bytes, including NUL and high bytes, through typed controller admission.

Payload and encoded limits are named separately. Both are currently 64 KiB. A valid payload can expand beyond the encoded limit in owner-side encoding; the owner rejects it before its pre-write hook or PTY write, with `ResourceLimit/EncodedInputBytes`. There is no truncation, splitting or synthesized accepted count. Multi-write expansion remains on hold.

The same request-ID activity/settlement admission book consumes the actual Runtime receipt. Positive completed writes report the real encoded count; a legitimate ignored encoding reports zero. Omitted settlement capture stays Unavailable, never legacy or a later marker resample. Persistence failure does not authorize input replay.

An exactly validated historical AcceptedInput receipt may finish after postcommit revocation. It keeps its original binding/inputId/kind and carries no continuing grant or current source data. Metadata, catalog and event responses retain postcommit authorization checks. Malformed/mismatched receipts and uncertain writes return InputOutcomeUnknown with retryInput false, never a fake zero or a NoWrite claim.

The Service keeps a bounded recent input-ID tombstone window, not a text archive or retry queue. This is not an unlimited exactly-once guarantee. A lost response never authorizes replay, including outside that window.

## Uploads

Terminal uploads use the existing private UploadInbox engine, not arbitrary PTY stdin or a second filesystem implementation. The upload actor is bound to the server connection, terminal generation and actual control epoch. Owner approval/connection fences and Runtime readiness are checked before staging and again at filesystem commit. The current adapter supports the eligible local normal source; it does not invent WSL/SSH path authority. File transfer IDs and terminal upload IDs remain separate.

## Verification and remaining gates

Verified at the checkpoint before the staged grant-custody migration:

- Combined protocol/client/Service check after the canonical KeyV2 flip.
- Service all-targets check, including compilation of four cross-crate key conversion tests.
- Remote and Service all-targets strict Clippy.
- Ten offline signed manager backend tests, including injected owner-boundary postcommit receipt/revocation cases.
- The approved private-output `manager_delivery` standalone target: five actual in-memory observer cases and two pure controller-reference/custody codec cases. These do not prove Native controller grants, IfFree, takeover or queue Full.
- Desktop and mobile TypeScript typechecks.

The four Service key conversion tests have **not been executed** under the Service-lib test restriction. Injected owner proofs are not owned-PTY acceptance. Controller delivery/input integration still needs its authorized native fixture; native platform capture, artifacts and end-to-end/default activation gates remain incomplete. Native capture completeness is not established by the full wire vocabulary or by source-table comparisons.

The staged caller migration consumes actual controller/observer grant offers from the shared Service custody path. Before adoption, a provisional delivery book captures recovery/data but never owns Release/Unobserve; central offer drop owns retirement. The Service holds a mutable validated book handle across central adoption and commits the book's adopted state in the same owner-serialized operation. Discarding an unadopted book refuses an adopted record. No separate manager claim ledger or receiver exists.

Remote owner requests now carry one cancellation token created and drop-guarded by the actual request future, including timeout/reply loss. Both direct and fact-worker commit sites pass that same token into the Service grant-recipient scope. Historical input receipt handling remains independent.

The migrated grant/token caller graph passes `cargo check --locked --offline -p shellfox-service --lib --tests` and scoped Remote/Service `cargo clippy --lib --tests -- -D warnings`. These are metadata checks, not Service-lib test linking or execution. The four focused Remote request-token tests passed execution; changed Service and standalone handoff tests are compiled only. Do not transfer the earlier seven-case standalone result to the changed eight-case target. Native controller/IfFree, grant uncertainty, cancellation, reply loss, shutdown retention and cleanup Full acceptance remain separate gates under the current test restrictions. Legacy attach behavior and default activation remain unchanged.

The Rust preferences consumer now uses the actual scoped private-leaf operations on the retained native control-child capability, with current-ownership checks before/after reads and retained one-shot publication custody. Approval inventory opens the exact `remote-v1` child through the compiled scoped child API. Neither visible preference publication nor V2 inventory grants approval authority: all native approval installation/management/writes remain gated pending crash-barrier and durability review. Native journals/uploads/editor initialization remains gated before IO; Go library path-state is separately unaccepted. Leaf fixtures and metadata checks do not prove Native/Go factory readiness or durable persistence.
