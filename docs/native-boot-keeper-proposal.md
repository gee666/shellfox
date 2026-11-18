# Native Boot keeper and common driver proposal

Status: **PLAN ONLY. Direction accepted, implementation NOT approved.** No Service/core-driver, public metadata API or writer changes follow from this document. The publication-owner contract and consuming transport transitions require independent review first. E owns the current Files check window; this document does not authorize Cargo or runtime work.

## 1. One common owner, not another Host

Reuse/extract the existing HostEndpoint native OS-thread driver into the common Service bootstrap layer used by Desktop, CLI and remote-composed serving. This moves custody outside caller-owned futures; it does not create another Service, Host, Go acquisition, runtime registry, parser, publication writer or cleanup scope.

Proposed Service-private roles, not compiled exports:

- `NativeBootDriver`: the existing native worker responsibility and bounded command/status transport.
- `NativeBootKeeper`: the sole original preparation/acquired-resource owner retained by that driver.
- Caller handles: bounded commands and waits, not replacement resource owners.

Keep thread-affine native listeners/guards on that native thread. Do not force BootGuard into a cross-thread Arc with unsafe Send/Sync assertions. The driver must not discard its keeper merely because a caller wait, channel, executor operation or thread diagnostic failed.

## 2. Actual outer placement

The native thread allocates its keeper on the synchronous stack OUTSIDE the executor `block_on`, all borrowed start/prepare futures, resourceful transition catches, cleanup futures and diagnostic catches.

The intended boundary is structurally:

```text
native driver thread:
    keeper = original owned bootstrap state
    executor = current-thread executor

    catch_unwind(|| executor.block_on(keeper.start_borrowed(...)))
    inspect result while SAME keeper and executor remain alive

    catch_unwind(|| executor.block_on(keeper.cleanup_borrowed(...)))
    retain SAME state on cleanup failure/interruption
```

A future that owns the keeper inside the catch is rejected. A new publication-owner field inside the current caller-owned `serve` future is also rejected. The keeper and original resources must outlive construction, polling, cancellation and unwinding of every borrowed operation.

Public async `serve*` facades must use the same retained driver/handle path. Fixing only Desktop while leaving CLI/headless BootGuard inside a caller-owned serve future leaves the drop gap open. No second `serve`, release/reacquire, cold fallback or second Host may be used to implement local availability or recovery.

## 3. What the keeper owns before native work

Before the first resourceful create/spawn/handshake await, the active keeper phase contains:

- Original private-directory plan, held ancestor/root handles and creation-retention owner.
- The SAME original OfflinePreparation/acquisition cleanup custody and controller registry.
- Every admitted partial actor/listener/remote startup controller needed for cleanup.
- The fixed location for the SAME original publication owner.

The publication-owner location is reserved before startup. Once the real control-child scope exists, construct the non-effectful original publication owner into that stable location BEFORE any publication preparation/native create. BootGuard uses its control view; it does not receive a newly prepared writer by value later.

On acquisition success, move the same acquired guard into the retained keeper synchronously before borrowed local start. Store partial resources before awaits. No take-to-resource-owning-local across await is allowed.

The keeper is not merely an anyhow payload. Typed errors/snapshots may retain a view of evidence, but the original driver still owns the actual cleanup state before diagnostics are created or returned.

## 4. Consuming transitions are an independent gate

The canonical transport signature/proof input is [Mulavarman's native producer input](../tmp/mulavarman-native-producer-input.md), owned by Mulavarman. [Tugh's Service requirements/supporting input](../tmp/native-transport-producer-input.md) records consumer obligations, including the pending real permit integration, and is not a second producer API authority. Neither document grants code approval or claims a compiled API. This remains the sole Driver plan.

Before any consuming preparation/acquisition transition, the keeper must retain an original cleanup-control view of the SAME producer owner/scope, or use a reviewed borrowed transition. A root lifetime token is not cleanup authority.

If a phase handle is consumed and the transition unwinds, usable original cleanup custody must remain in the keeper. A local return value, a local error Box, or a fresh controller reconstructed afterward does not satisfy this. Do not assume a nominally synchronous transition is panic-safe without reviewing its actual allocations, callbacks and ownership movement.

Mulavarman's independent transport audit makes the boundary precise:

- `bind_root(self)` and `into_acquired(self)` are synchronous moves, not async gaps by themselves. The current Service `mem::replace(..., Transferred)` path nevertheless removes actual custody into locals. An outer keeper does not make those locals recoverable on unwind. Use reviewed borrowed in-place transitions or retain a phase-independent ORIGINAL cleanup-control view first.
- `Offline` acquisition borrowing the retained attempt already registers its original child before pipe/task work and awaits. Cancellation requests release; it is NOT confirmation of child exit. Preserve that original registration and require its real exit/wait proof.
- Compatibility `HostOwnership::acquire` is unsuitable for the new Service handoff: path recapture and resource-bearing failure returned only as an error do not preserve the Service's original root/custody contract. Do not route around the reviewed retained path through this convenience API.
- Network-helper `Process::spawn` registers `PendingHelper` only after pipe/task setup. Its post-spawn return/capture gap is separate. Native remote initialization currently stays behind the static authority gate before reaching this path; this plan does not approve it.
- `RootLifetimeGuard` retains directories only. It is neither an owner nor a phase-independent cleanup view and cannot replace either one.

Native spawn/create admission still needs review at every resource boundary. Registered uncertainty must precede native effect; an empty registry after an interrupted admission does not prove that no child/object was created. No PID rediscovery or path-based authority reconstruction is permitted. The outer keeper is necessary, NOT sufficient. No driver implementation against the current consuming APIs is approved.

This dependency may require an additive original-owner/control-view producer API. It is not permission to introduce another owner lifecycle. No implementation is approved until the transport owner and independent reviewer accept the exact retention contract.

## 5. Resourceful startup inventory

The same rule applies at each boundary, not just endpoint writing:

| Boundary | Required original custody before effect/await |
| --- | --- |
| Permission helper / Go acquisition | Original preparation registry and retained cleanup control view |
| Private directory creation | Original parent/identity and pre-reserved success/failure slot |
| Actor thread / Host startup handshake | Retained actor-start controller/envelope, not only a future-local returned ServiceOwner |
| Local listener creation | Original listener admission/uncertainty and returned handle before postchecks |
| Remote initialization | Existing retained RemoteHostSetup envelope and partial resources before awaits |
| Endpoint preparation/write/publication | SAME original publication owner/capture storage before creator entry |

Existing retained components should be reused. Any boundary still returning its sole resource by value before the keeper can capture it remains a gate. This plan does not declare those boundaries already safe.

## 6. SAME publication attempt and lock contract

E owns the sole Files core Owner/control-view API proposal: [private-publication-original-owner-proposal.md](private-publication-original-owner-proposal.md). [Eva's owner-view proposal](private-publication-owner-view-api-proposal.md) is consumer/migration/requirements input linking E's canonical API, not another core API authority. This Driver plan consumes the jointly reviewed SAME-owner hook; it does not define another CAS writer. Endpoint is one-shot first; DeviceColors reuse, retirement and CAS remain separate review.

Raw `CreationCustody`, creation-entered uncertainty and the actual optional native FD must be fields of the original persistent slot BEFORE creator/postcheck callbacks can run. A native create out-parameter must point into that slot, not a local Box later returned into it. Moving the actual FD/full ID between success fields of the same slot must be infallible, with no allocation, callback, snapshot or fallible gap.

Seal/revoke ATTEMPT admission before cleanup. Do not globally revoke Files/server authority as a substitute for attempt sealing.

All attempt mutators, including cleanup and preserve, use global MUTATIONS -> slot order. Snapshot takes only the slot and copies owned diagnostics; it does not call a MUTATIONS/native-commit callback or leak a mutex-owned reference. No alternate slot -> MUTATIONS path is approved.

A control failure owns its typed diagnostic snapshot AFTER custody has been retained. Its error borrow may refer only to that owned snapshot, never to unlocked inner state. Snapshot allocation/formatting failure must not drop the original attempt.

## 7. Panic and diagnostic outcomes

An interrupted start/prepare/transition/cleanup becomes retained unknown custody/ownership, not ordinary safe failure or NoOwner. Preserve the original controller/root/publication owner before reporting it.

Do not infer NoCreate, NoWrite, NoPublication, NoOwnedAttempt or process absence from None, an empty slot/registry, EOF, thread completion or a caught panic. The current minimal `CustodyUnknown` report for attempted preparation without a stored receipt remains conservative until the real ownership hook exists.

Diagnostic callbacks are separately caught and cannot unwind the owners. Logging failure does not discard state. Do not run arbitrary callbacks while a protected publication slot is held. A panic catch is not a cleanup proof and must never become a generic catch-to-clean conversion.

The same keeper drives borrowed cleanup. Errors or further panic keep it retained. Only original actor/helper/controller confirmation and explicit artifact disposition may advance their respective reports. Filesystem preservation is separate from process NoOwner and artifact absence.

Keep the executor alive across the catch where possible. An unusable executor does not authorize dropping live async resources or replacing their owners. Runtime replacement/teardown with unconfirmed resources is separately reviewed; until then the keeper stays retained/unknown.

## 8. Future drop and caller lifetime

- Cancelling a borrowed ready wait drops only that wait. It is NOT Stop or permission to drop the keeper.
- HostClient/view detach semantics stay unchanged and do not become PTY shutdown authority.
- An explicitly cancelled borrowed startup operation releases its borrow, not its original owner. The driver records interruption and retains inspection/cleanup custody.
- No await exists in the native capture segment. Any possible create and returned FD are already recorded in the original slot before the next cancellation point.
- Explicit Stop and the existing unclaimed-ready normal Stop path operate on this same original driver/keeper. They do not reacquire or create another server.
- Closing a command/ready/completion channel is not process absence or NoOwner. The driver does not destroy its state because its caller-facing future was dropped.

Any new application-owner abandonment policy must be reviewed explicitly. This plan does not silently turn ready-wait or view drop into host termination. The common driver must remain able to retain/clean original resources even when the cancelled caller no longer polls a future.

## 9. Boundaries that remain separate

This proposal adds no cleanup-authority bool, new wire/IPC privilege, filesystem CAS guarantee, Host/VFS confinement, approval authority, parent ACL relaxation or runtime permission. Current root privacy, native mutation restrictions and the exact local-only/remote-unavailable contract remain unchanged.

Windows namespace pins and original chain lifetime retain only their documented proof. They do not establish Unix confinement or permit privileged filesystem mutation. Original helper process ownership remains distinct from publication/artifact cleanup.

Before implementation: review the common-driver placement, all consuming-transition control views, original-slot publication hook, actor-start retention and caller-drop routing together. Then publish exact compiled APIs and a coherent consumer batch. Metadata/lint READY alone is not runtime, panic-injection, cancellation or cleanup acceptance.
