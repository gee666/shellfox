# Launch privilege and source sharing boundary

Common terminal/Runtime FACTS and the standard PTY producer are implemented. Raw factories use the frozen `spawn_with_launch_binding` integration hook. This is not a share grant or a new wire contract.

## Existing evidence we can reuse

- Standard Windows PTY startup already authenticates the original supervisor through a private pipe, then retains a query-only handle for the exact launched child. The handle is checked against native FILETIME birth and the exact owned Job before READY. `inspection::OwnedInspection` holds the needed child/Job handles.
- Windows raw automation retains the original `std::process::Child` supervisor handle and Job. The current raw protocol has no target-child handle/birth transfer. The trusted raw supervisor uses ordinary `Command::spawn` with no alternate-token or elevation API. A narrowly reviewed factory-inheritance attestation could use that documented primary-token inheritance; it must be labelled as such. Prefer the existing private child-proof pattern if direct target-token evidence is required.
- Program/resource leases prove executable/resource identity and lifetime. They do not prove process privilege.
- The managed elevated channel's READY PID/birth and a fake fixture's purpose do not prove a real elevated token. Its real factory remains gated. Do not infer elevation from creation intent, a profile name or a message marker.
- Rust currently has no original-handle token-elevation reader. The old TypeScript owner-SID helper uses PID discovery and is not an ownership/privilege authority to reuse here.

## Concrete producer boundary

Terminal-local opaque, non-Serde `NativeLaunchEvidence` constructors are private to actual owned factories. It records:

- Native owner/process identity and exact birth, retained from the original handle proof.
- Factory purpose, fixed by the implementation, not a caller/profile string.
- A sampled launch privilege fact: Normal, Elevated or Unknown.
- The evidence basis, such as OriginalTargetPrimaryToken or ReviewedFactoryPrimaryTokenInheritance.
- An opaque source-binding identity, not a numeric PID or executable path.

Capture only through an original process/supervisor handle already checked for the owned Job and birth. Capture borrows those handles, closes its TOKEN_QUERY handle immediately and retains no extra Job/process/token handles in the sealed receipt. The original owner alone retains its domain; a published parser retains only immutable initial facts/native birth and its private binding. Never open an arbitrary PID to manufacture this capability. No WMI/global scan, token adjustment, UAC, debug privilege or PID/path fallback belongs in the producer.

On Windows use `OpenProcessToken(TOKEN_QUERY)` and checked `GetTokenInformation` on that handle. Token elevation is a fact, not a claim that the account lacks administrator membership. Capture elevation type and integrity as sanity evidence; inconsistent/high-integrity cases that cannot justify Normal stay Unknown rather than being labelled non-elevated. No ordinary-source default is permitted. An administrator host may cause the standard PTY/raw target to inherit an elevated token.

For PTYs the direct authenticated child handle is already available. The approved raw first slice uses the reviewed standard-factory inheritance basis below; the direct-child alternative remains a possible future strengthening:

1. Extend the private raw supervisor handshake using the existing child-proof pattern, then query the original target handle; or
2. The implemented approved first slice attests the fixed standard factory's same-primary-token launch contract using the original supervisor handle captured after exact Job assignment and before GO, confirming the receipt only after successful READY. It exposes ReviewedSupervisorPrimaryTokenInheritance, not a fabricated direct-child observation.

`RawProcess::spawn` remains unbound and yields no inferred receipt. `RawProcess::spawn_with_launch_binding` may capture a pending standard-factory observation after exact Job assignment and before GO, confirming it only after successful READY. Until that actual factory receipt is bound to its exact Runtime, ExternalBytes stays Unknown. Generic external byte streams have no process evidence at all. Unsupported OS/native helper paths stay Unknown and keep their existing execution gates.

## Bind the raw factory to the reserved source

An external parser is created before raw launch. `ExternalSource::take_launch_binding` gives trusted bootstrap one non-Serde binding from that parser. The non-clone binding is consumed by the frozen `RawProcess::spawn_with_launch_binding` overload, and its one-shot `take_launch_evidence` supplies the receipt to `ExternalSource::bind_launch_evidence`. Runtime checks both the private instance seal and fixed factory purpose. Pass it into the actual raw factory. The factory returns opaque evidence carrying that same private binding after successful owned startup. The Runtime/Service can then bind the evidence only to that reserved source identity and generation.

Do not expose `set_privilege(Normal)`, accept a client enum, use the run PID/path as the binding, or attach an old run's evidence to a reused request ID. The binding is correlation only, never permission to launch or share. Preserve it with a published parser when its raw RPC record retires.

## Sharing is separate

Keep three things distinct:

1. Descriptive initial-launch privilege evidence.
2. Trusted native source-share admission, issued for an actual registered factory purpose and exact source/generation.
3. The authenticated peer's existing ReadOnly/Write permission.

A V2 attachment must satisfy the owning Service's current share policy and peer permission. `SourceKind::ExternalBytes`, `profile.elevation == None`, creation intent, output text and Source Closed are not share grants. Elevated/Unknown launches should remain local-only under the initial conservative policy unless a separate reviewed native policy explicitly says otherwise. A read-only source never gains stdin/resize rights just because its launch was Normal.

Use a concrete Service source-evidence registry, not an unimplemented generic provider facade. The existing V2 `Fact`/Unknown representation can report unrecorded/unavailable evidence internally. Server-side share gating can be implemented before adding a privilege field to a wire schema. No new wire field is proposed by this document.

The current profile filter must not be called launch proof. Replace or supplement it only when real factory evidence and source-share admission are connected. Facts copied into a watch/DTO must not substitute for the opaque owner registry capability at admission.

## What this does not promise

This describes the initial owned launch. It cannot promise that every later interactive command, descendant, SSH/container context or user-approved elevation stays non-elevated. A static Normal launch fact is not an all-future-session security guarantee. Source sharing remains an explicit data-sharing decision and must document that scope.

## Native tests without protected targets

- Query the current test host token through its actual handle, then query a genuinely owned fixture child. Compare observed values, never hardcode Normal. If the host is elevated, the ordinary inherited child must not be reported non-elevated.
- Exercise only these owned fixtures, with no UAC or account/network actions. A mock elevated channel remains Unknown; mapping-unit tests are not native elevation acceptance.
- Reject stale/wrong source bindings, missing original-child proof and token-query failure. Keep permission independent from descriptive facts.
- Prove published final parsers retain their source evidence across raw-record eviction, and reused request IDs cannot acquire an old parser/grant.
- Do not claim Linux/macOS privilege acceptance from Windows or model tests.
