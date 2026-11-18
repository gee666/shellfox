# Observer-capable manager profile (proposal)

Status: design review only. No protocol, allowlist, mobile API, or transport behavior is changed by this document. Local automation and native Show do not establish remote/mobile parity.

## Why a new profile is required

Strict v1 terminal delivery and ACK require a real owned controller lease. An observer must not receive a fabricated lease, and a blocked state must not carry terminal bytes. V1 session facts also require adapter/shell values that an arbitrary raw executable cannot supply truthfully.

Changing an enum, making a required field nullable, or using the same 43 method names with different payloads would still change v1. Keep its DTOs, validation, and allowlist intact.

## Negotiation

Proposed profile identifier: `shellfox.manager.observer-v2`.

- Negotiate a supported profile explicitly on the authenticated connection before manager request decoding. Bind the selected profile/version to that connection's approval epoch and authentication transcript.
- A client that offers no profile remains strict v1. Do not infer support from platform, native Show, or a successful v1 probe.
- A rejected/unknown profile fails before dispatch. Reconnection renegotiates; a lost response never authorizes replay of a mutation.
- Keep the current 43 remote method strings. Each profile has separate request/response schemas and conformance tests. Local automation's six methods and the private desktop-launch namespace remain outside that table.
- Specify downgrade handling and the exact handshake schema before implementation. Do not add fields to the current strict v1 handshake speculatively.

## Three independent authorities

V2 needs distinct types rather than an `Owned` flag with exceptions:

| Authority | Allows | Does not allow |
| --- | --- | --- |
| Delivery observer | Receive/recover/ACK that view's bounded delivery | Input, process control, canonical layout changes |
| Input controller | Existing checked terminal input/control operations | Authority over another view/generation |
| Layout owner, if negotiated | Change the one canonical external parser grid | Resize a raw process/PTY or gain input control |

An observer delivery capability should bind an unguessable view ID, observer epoch, immutable source identity/generation, and approval epoch. ACK must use that delivery capability and its **delivery** sequence. Input handlers must independently require a genuine controller lease; accepting an observer token anywhere on an input path is a protocol error.

Keep raw source sequence separate from segmented display delivery sequence. Legacy automation display conversion can produce no display bytes for an accepted raw chunk, or produce final display bytes during decoder flush. Represent absent delivery ranges explicitly and include final-flush mapping in recovery/finish tests.

Observer layout defaults to fixed canonical-grid clipping plus a local viewport. Current `ResizeExternal` changes the single canonical parser geometry; it is not independent per-view reflow. Do not describe it as such or expose it through delivery authority. If a layout-owner capability is not agreed, remote observers cannot resize the canonical grid.

Detach/revoke closes only the view capability. Raw stop and metadata deletion require their own explicitly authorized lifecycle operation. A parser/source-finished or parser-closed event never proves original-process cleanup.

## Truthful source and creation facts

Proposed v2 facts distinguish:

- Source kind: owned PTY, owned raw process with external-byte presentation, or unavailable/unknown source. A tag is descriptive, not executable or cleanup authority.
- Creation provenance: verified catalog intent with its actual adapter/shell selection, or explicit unknown provenance, such as an arbitrary executable with no catalog selection.
- Source completion, final display flush, parser closure, and raw cleanup: separate states/evidence. Only the retained original raw owner can prove process-tree cleanup; PID, marker, metadata, and runtime phase cannot.

Do not fabricate `EmbeddedPty` or `Pwsh` facts for raw execution. Do not backfill absent historical facts from current settings, another tab, or the executable name. Reuse the existing `automation-pipe` compatibility marker for read-only UI classification, but derive authority from the actual runtime/owner, never that marker.

## V1 compatibility policy

No silently filtered partial inventory. If a requested strict-v1 snapshot contains required facts or observer delivery that cannot be represented:

1. Return an existing typed, non-retryable incompatibility/upgrade error, with bounded unsupported coverage information (count/IDs), not invented facts or an invalid DTO.
2. Keep independently projectable operations available; do not let an unrelated unsupported row poison every source lookup or trigger a projector panic.
3. Surface the incompatibility at profile/manager readiness and at the affected operation. Do not assume old clients display probe reasons.

There is no truthful v1 raw row under the current required fact fields. Returning normal rows while excluding raw rows requires a separately negotiated partial-catalog mode **and an actually surfaced notice**. Without that agreement, fail the affected inventory explicitly. Preserve genuine historical facts without filtering or relabeling them.

## Required tests before wire work

- Strict v1 golden payloads and the exact 43-method allowlist remain unchanged; v2 payloads are rejected on a v1 connection.
- Negotiation/authentication epoch, unsupported profile, downgrade, reconnect, and cancellation tests.
- Observer delivery/recovery/ACK without a controller; every input/control path rejects observer tokens.
- Stale view epoch/generation, revoked approval, cross-view ACK, future ACK, bounded credit, and slow observer tests.
- Fixed-grid observer clipping; separately authorized canonical layout, with no PTY/raw resize or input escalation.
- Per-stream decoder fragments, zero-display chunks, final decoder flush mapping, full primary/history recovery, and late observation after source finish.
- Original raw cleanup independent from parser failure/close; retained failed bytes without fabricated EOF/Finish/ACK.
- Mixed normal/raw inventory, unknown creation facts, explicit v1 incompatibility, and independently projectable normal operations.
- Desktop, headless, macOS, Windows, Linux, Android and iOS client conformance before claiming parity. Native ownership gates remain fail-closed where platform proof is absent.
