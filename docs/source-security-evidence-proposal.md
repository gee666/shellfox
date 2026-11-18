# Trusted source security evidence (proposal)

Status: review before implementation or wire changes. This is not a generic provider interface or a promise that current source-kind/profile tags are security evidence.

## Existing boundaries inspected

- `shellfox-terminal/src/raw_process_windows.rs` retains the original supervisor `Child` and exact non-breakaway Job. It assigns/verifies the Job before GO. The trusted supervisor spawns the requested program without token substitution.
- `shellfox-terminal/src/windows.rs`, `supervisor.rs` and `inspection_gate.rs` provide the corresponding owned PTY startup boundary.
- `inspection.rs` / `inspection_windows.rs` already retain query-only authenticated root/Job capabilities, tied to native birth, membership and startup proof. Their current public capture API has process/argv evidence, **not token-elevation evidence**.
- `elevated/transport.rs` describes authenticated elevated-peer original-handle cleanup. It does not currently expose the peer's launch token as a source fact.
- Service `pipe_identity.rs` and `system_environment.rs` query peer/current-user identity for other purposes. Those are not source launch-privilege APIs.
- `remote_privacy.rs` uses existing v1 profile policy. Preserve that policy and DTO shape; do not reinterpret `profile.elevation == None` as a new v2 proof that an actual process is non-elevated.

## Concrete capture points

Add a sealed native launch-security snapshot to the **existing** factory-owned session/process object, not an injectable permissive provider:

1. For standard Windows raw/PTY startup, query the original supervisor process handle after exact Job assignment and before GO. The verified factory's no-token-substitution spawn contract ties that initial token to the target launch.
2. Alternatively, extend the already-held authenticated root inspection capability to query its pinned handle. Do not reopen a PID or inspect a path/name to obtain privilege authority. Document the capture point rather than claiming a pre-launch guarantee from a later observation.
3. An elevated helper needs an extension of its authenticated original-peer boundary. Factory intent alone can deny sharing, but cannot manufacture a successful token measurement.
4. Unsupported platform/protocol, failed query, incomplete binding, or an unavailable original capability produces **Unknown**, not Normal.

A Windows implementation can use `OpenProcessToken(original_handle, TOKEN_QUERY)` and bounded `GetTokenInformation` calls for `TokenElevation`, elevation type and conservative integrity/UI-access checks. Borrow the original factory-owned handle; close temporary token/query handles before returning the sealed snapshot. Retain no credential/token handle in a public DTO. Conflicting/incomplete measurements remain Unknown. In particular, an ordinary launch from an administrative Host may inherit an elevated token and must be reported as Elevated.

## Binding to the real owner

Terminal/Runtime can expose read-only sealed evidence already associated with its own immutable Runtime identity after successful startup. For raw processes, `AutomationRuns` already binds the reserved Runtime identity before launch; expose its native capture only through that exact run/source binding. A caller must not construct a binding from a PID, `SourceKind`, marker, selected row, executable path or current settings.

Service stores the capture with the matching generation and revalidates current registry/binding before exposure. A historical row without a live owned generation has no current launch-security proof. Replacement/restart needs a new capture; never reuse a previous generation's Normal result.

## Handle custody

A read-only duplicate Job handle still affects `KILL_ON_JOB_CLOSE`. Do not store a new Job/token/inspection capability in a long-lived published fact receipt. Capture through the original owner's existing domain, then retain only the sealed fact/binding. The owner still performs its existing explicit termination/wait/cleanup proof; a privilege snapshot neither keeps a nonempty Job alive nor releases it.

Tests must cover owner drop, capture/query failure, READY failure and published-parser retirement. Those tests prove custody/cleanup of the original process group, not just that a token query returned a value. No PID fallback is allowed when the owned capability is unavailable.

## Facts and permission are separate

Mechanical facts:

- Initial launch privilege: Normal / Elevated / Unknown.
- Capture domain and factory purpose: local interactive launch, raw automation, local SSH client, authenticated elevated helper, or controlled fixture.
- Exact immutable source binding and capture stage.

Permission:

- Existing manager policy determines whether even saved metadata/IDs may be disclosed.
- A trusted factory/native owner share decision and current authenticated approval determine live delivery eligibility.
- Raw automation starts local-only; a measured Normal token does not turn it into a shared terminal automatically.
- Unknown privilege/share evidence means live delivery unavailable. Authorized metadata may still state unknown/unavailable explicitly. Unauthorized metadata must not be exposed, even as an ID-only unavailable row.
- A local `ssh.exe` token says nothing about the remote guest shell's privileges. Do not infer guest privilege from username, profile marker, or the local token.

These decisions must be concrete owner-held records, not a `SourceVisibility` façade returning guessed defaults. Observer delivery authority remains separate from input and layout authority.

## Limit of the fact

The fact describes the initial owned launch token. It cannot promise that every future in-session program remains non-elevated, that the program never uses an administrative broker, or that `sudo`/UAC/network services cannot be requested later. Do not label it “session can never elevate.” If policy requires a stronger claim, that source is unavailable until the required evidence exists.

## Tests before enabling live attach

- Query a real owned Windows fixture through its original handle; compare against the actual inherited/current test-token state without requesting UAC. Do not hardcode Normal: CI may itself be administrative.
- Verify Elevated when the real captured token is elevated; a fake elevated helper only tests protocol behavior and must not be advertised as an actual elevation test.
- Query failure, unsupported OS, stale generation, wrong source binding and resource-role mismatch remain Unknown/denied.
- Normal launch plus local-only factory purpose still denies live sharing; permitted metadata remains visible only under existing policy.
- Admin Host inheritance never becomes Normal merely because the selected profile requests no elevation.
- Current approval revocation, late response and source replacement cannot revive a prior share/elevation decision.
- No PID enumeration, UAC prompt, real password, unrelated process inspection or production endpoint is needed for these owned tests.
