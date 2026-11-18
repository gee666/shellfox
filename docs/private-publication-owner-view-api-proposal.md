# Publication consumer/migration requirements input

Status: **CONSUMER INPUT ONLY — not core API ownership, compiled publication, implementation approval or adapter READY.** E alone owns the Files producer/core owner-control-view API in [the canonical original-owner proposal](private-publication-original-owner-proposal.md). This document must not maintain alternative signatures, owner/control types, writer state machines or lock APIs.

Read E's full canonical proposal before using this input. Its Endpoint one-shot scope, unchanged by-value compatibility APIs, original-slot capture, owned diagnostics and all-mutator lock order are the starting contract. DeviceColors reuse/retirement/CAS is separately deferred.

## Canonical dependencies and owners

- Files publication producer/core API: [E's original owner/control-view proposal](private-publication-original-owner-proposal.md). E publishes the actual agreed/compiled exports; consumers do not infer methods from this input.
- Transport signature/proof input: [Mulavarman's owned producer input](../tmp/mulavarman-native-producer-input.md), DIRECTION ONLY. No second transport API table is maintained here.
- Sole Driver/outer-keeper plan: [Tugh's owned native Boot keeper proposal](native-boot-keeper-proposal.md), PLAN ONLY. Service supporting notes are requirements input, not another transport or Driver authority.

Consuming-transition, native capability-permit integration and original publication capture remain joint review gates. No Driver/Endpoint/transport/Files implementation is approved by these documents.

## 1. Endpoint is the first consumer, one-shot only

The original Boot/Endpoint envelope must contain E's actual persistent owner before creator entry, not an empty receipt slot that is populated only after prepare returns. Use the same existing Owned/Scoped publication implementation and the same attempt slot; no endpoint-specific CAS writer.

Required capture facts:

- Raw CreationCustody and uncertainty storage are installed in the ORIGINAL slot before NT.
- The native out-parameter points directly into that slot. The actual optional FD is retained before full-ID queries, privacy/binding postchecks, cancellation observation or diagnostics.
- Success moves the same FD/full ID between states within the same slot through an infallible, no-allocation/no-callback/no-fallible window. No local return-to-owner gap.
- Returned typed failure and unwind both leave that same slot reachable. A locally retained Box inside prepare is not outer retention.
- Attempt sealing precedes cleanup/preserve. It does not globally revoke Files/server authority. Actual owned receipts, not current privacy, authorize owned cleanup.

Do not move a by-value prepared object back into BootGuard and claim the ownership gap fixed. Existing compatibility calls/tests remain separate and unchanged. Endpoint remains blocked until the real outer lifetime and original-slot hook both exist.

## 2. Actual outer lifetime is a separate consumer obligation

Tugh's canonical plan owns the exact guard/controller/root placement and borrowed start/catch/future-drop boundaries. The keeper must actually outlive future construction/polling/drop and executor unwinding. Merely adding a field inside serve is insufficient.

Mul's canonical input requires the same Arc-backed Bound root and phase-independent original cleanup view, original scope admission AND original-controller fence, pre-spawn uncertainty/preinstalled original child storage, and producer-private no-child evidence. Do not replace those with RootLifetimeGuard, a cloned lost token, compatibility path recapture, cancellation/release request, empty registry or generic catch→clean conversion.

The concrete Service `mem::replace(..., Transferred)` extraction is a custody gate. Synchronous consuming functions are not automatically async gaps; they still need the actual source-reviewed in-place/control-view contract where extraction can unwind. The separate network post-spawn registration gap remains behind the existing static remote gate.

Caller diagnostics occur only after original state is retained. Endpoint reports attempted-plus-empty capture as CustodyUnknown, never NoOwnedAttempt/NoOwner/no artifact. Filesystem disposition, controller/process exit confirmation and publication durability remain independent facts.

## 3. Locking, diagnostics and cleanup requirements from consumers

Follow E's canonical all-mutator order: actual global MUTATIONS → original publication slot (including cleanup and preserve). An outer actor serialization gate must not create a reverse slot→MUTATIONS route.

Snapshot takes only the slot, invokes no MUTATIONS/native callback and returns owned typed diagnostics. Failure.error borrows its own retained snapshot, never unlocked mutex-inner state. Snapshot allocation/formatting failure cannot release native custody.

An unresolved/interrupted/sealed attempt blocks another preparation. No stage-name replay, new owner, TTL erasure or hidden success-retirement path. Endpoint is one-shot; DeviceColors repeated-save behavior is NOT approved through this input.

Poison remains failure/quarantine. Inspection may recover conservatively without clearing poison or regaining ordinary authority. Cleanup/preserve must follow E's reviewed lock/error contract; this input does not invent a second recovery lock helper. Failure to complete disposition retains the original actor/owner and unknown evidence, not a cleanup proof.

## 4. Pending native capability-adapter commit-permit contract

This section is a **joint integration requirement**, not a Files/transport API definition. Exact new producer/adapter exports remain to be reviewed and published by their owners.

### Actual current limitation

Current Files `NativeCommitFence::check() -> ()` samples a restriction. Service OwnershipFence currently checks lost while retaining directory/creation lifetime. It does not hold/drain canonical native admission through a filesystem syscall. Current adapter/scoped commit callbacks run checks after acquiring MUTATIONS; inserting a fresh scope-enter there would introduce reverse lock order and is forbidden.

### Required real guard lifetime

At native-owned operation entry, obtain the canonical transport admission from the retained SAME original acquired-controller fence before waiting for Files MUTATIONS or taking the slot. The Files adapter must keep the actual guard alive through the bounded synchronous native operation, final checks and immediate outcome capture. Checking and dropping it before the syscall is insufficient.

The adapter must retain/reach that gate in every derived native capability and preserve all original scope/RW/revocation/current-privacy/other commit restrictions. A lost token, cached acquisition bool, RootLifetimeGuard or fresh path/PID reconstruction cannot supply it. Do not fake a Go fence for independent local configuration authority. Do not silently drop the real gate while narrowing a native-owned capability.

No guard crosses await or arbitrary callbacks. Acquire a separate admission for each bounded prepare/write/publish command; the publication owner/attempt persists between them. Native reads/queries/reopens/child handoff need an explicit inventory too—not merely final rename. This is ordinary native-use admission, not cleanup authority.

Proposed full lock order to review:

```text
existing outer actor authorization/serialization, if any
    -> canonical scope admission
    -> original-controller checks in the canonical scope→control direction
    -> actual Files MUTATIONS
    -> publication slot
    -> bounded native syscall and immediate recorded outcome
```

Existing actor/approval ordering must be audited against this order before acceptance. No original-control→scope back edge, slot→scope acquisition or under-MUTATIONS re-entry is allowed. A held-permit context, not an entering callback, must reach the existing final commit checks. Multiple gate composition/deduplication/order must be defined centrally or rejected, not guessed by consumers.

### Seal and in-flight cleanup

Scope seal runs without a Files MUTATIONS lock, publication slot, native admission or controller lock held by that caller. It closes new admission and drains the already-held canonical native segments according to Mul's producer contract. It must never wait while holding a lock those segments need.

Service-controlled release/loss transitions must follow the reviewed same-scope/original-controller discipline. External process exit is not frozen by a Rust lock. Recheck original-controller status at the actual native boundary; post-effect loss/uncertainty records the real possible/actual outcome and does not become NoWrite or a replay. No Go/filesystem atomicity promise is derived from boolean health checks.

Cleanup ordering remains separate:

1. Outside Files locks, seal/drain the canonical native admission using the original cleanup view.
2. Under E's global MUTATIONS→slot order, seal only the publication attempt and apply its actual owned cleanup/preservation receipt.
3. Confirm process/controller resources and filesystem artifact disposition separately.

Exact owned cleanup cannot depend on reacquiring ordinary native admission after seal and cannot use its receipt to publish/write/grant another scope. Snapshot remains slot-only. Existing FileService/global server revoke is not a substitute for attempt sealing.

The bridge must keep the real permit non-transferable across await, respect actual thread affinity and require no unsafe Send/Sync. If the actual producer cannot support a proposed adapter shape, stop for review rather than fabricate a permit trait or another driver.

## 5. DeviceColors and journal migration remain deferred

DeviceColors current path-compatible constructor is unchanged. A candidate current-private root check/bounded read does not establish save/CAS/retention/reuse safety. Repeated attempts, retirement, same-bytes/metadata versions and failure retention need the separate accepted producer contract before consumer edits/wiring.

Journal/editor/uploads Stage 2 is blocked for BOTH PathBound and Native. Private metadata scope/current privacy is not actual user-stage recovery authority or historical V1 identity. No V1 destructive recovery/rewrite/.next cleanup/lease fallback follows from this input. Accepted actual user-stage authority and V2 schema remain prerequisites.

NativeDesktop's separate static RecoveryAuthorityPending safety gate is not an implementation of this publication adapter. Its focused negative tests/metadata checks establish only that gate, not lifecycle/privacy/runtime acceptance.

## 6. Evidence limits and next review

Do not resummarize candidate checkpoint tests as actual mutators. RootCheck Stage/Write/Commit currently have checkpoint coverage, not implemented writer acceptance. The native-binding Unsupported case was unexecuted in the reported scope; query-failure/partial native-return branches include source-only evidence. Hook errors/panics are not general caller cancellation; the candidate has no real cancellation input and revocation waits for MUTATIONS. Do not add an unwired token or fake native fixture to enlarge coverage claims.

Keep authoritative E/W counts and prerequisites intact; no doubled test totals or promotion claim. Darwin compilation is not Darwin runtime acceptance; a timed-out test remains failed/unconfirmed and PID ownership evidence retains its stated limitations. W's source review of E is not independent review of W's own guard.

Pending review must establish the actual original-slot capture, outer keeper/future-drop contract, held commit-permit adapter, current privacy/binding and precise CAS/receipt behavior together. No new Rust/Go/runtime tests, public Windows route, writer/consumer/default activation, Service-lib execution or Driver implementation is authorized by this input.
