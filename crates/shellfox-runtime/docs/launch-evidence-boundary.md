# Launch privilege and source sharing boundary

`NativeLaunchEvidence` records how privileged a terminal or raw process was when it started. It is a description, not a share grant, a process-control right or a wire field. The types live in `crates/shellfox-terminal/src/launch_privilege.rs`.

## What the evidence holds

`NativeLaunchEvidence` is opaque and not Serde. Only the owned factories can construct it. It records:

- the native process identity and exact birth, taken from the original handle;
- the factory purpose (`LaunchFactoryPurpose`), fixed by the implementation, never a caller or profile string;
- the launch privilege: `Normal`, `Elevated` or `Unknown`, with a `LaunchUnknownReason`;
- the basis (`LaunchEvidenceBasis`), such as `OriginalPinnedChildToken`, `ReviewedSupervisorPrimaryTokenInheritance`, `OriginalPinnedPosixCredentials` or `OriginalDarwinCredentials`;
- an opaque source-binding identity, never a numeric PID or executable path.

Evidence is captured only through an original process or supervisor handle that has already been checked against the owned Job and its birth time. Capture borrows those handles, closes its `TOKEN_QUERY` handle at once and keeps no extra Job, process or token handle. The producer never opens an arbitrary PID, scans WMI or the process list, adjusts tokens, triggers UAC, uses debug privilege or falls back to a PID or path.

## Windows

The producer calls `OpenProcessToken(TOKEN_QUERY)` and checked `GetTokenInformation` on the original handle. Token elevation is a fact about that token, not a claim that the account isn't an administrator. Elevation type and integrity level are checked for consistency. A high-integrity or inconsistent token that can't justify `Normal` stays `Unknown`. There is no default of `Normal`. A host running as administrator can give standard PTY and raw targets an elevated token.

PTYs: standard Windows PTY startup authenticates the original supervisor through a private pipe and keeps a query-only handle for the launched child. That handle is checked against its FILETIME birth and the owned Job before READY. `inspection::OwnedInspection` holds the child and Job handles.

Raw processes: the raw supervisor keeps the original `std::process::Child` handle and Job, and starts the target with plain `Command::spawn`, with no alternate token or elevation API. The raw protocol doesn't pass a target-child handle back. So `RawProcess::spawn_with_launch_binding` records the supervisor's primary token after exact Job assignment and before GO, and confirms it only after READY. The basis says so (`ReviewedSupervisorPrimaryTokenInheritance`) rather than claiming a direct child observation. Passing a child-proof handle through the raw handshake, as PTYs do, would make this a direct observation.

`RawProcess::spawn` stays unbound and produces no evidence. Generic external byte streams (`ExternalBytes`) have no process evidence and stay `Unknown`, as do unsupported OSes and helper paths.

Program and resource leases prove executable identity and lifetime, not privilege. The administrator helper's READY PID and birth don't prove an elevated token either. Never infer elevation from creation intent, a profile name or a message marker. The old TypeScript owner-SID helper found processes by PID and is not a privilege authority.

## Linux and macOS

See [Unix PTY support](../../shellfox-terminal/UNIX.md#privilege-policy). Linux reads the pinned controller's UIDs, capabilities and `no_new_privs` before GO (`PosixLaunchFacts`). macOS records matching real, effective and saved UIDs at launch and makes no claim about later execs.

## Binding evidence to a source

An external parser exists before its raw launch. `ExternalSource::take_launch_binding` hands trusted bootstrap one non-clone, non-Serde binding from that parser. `RawProcess::spawn_with_launch_binding` consumes it, and after a successful start `take_launch_evidence` returns evidence that carries the same binding. `ExternalSource::bind_launch_evidence` checks the Runtime instance seal and the factory purpose before accepting it. The Service can then attach the evidence only to that reserved source and generation.

Don't add `set_privilege(Normal)`, accept a privilege enum from a client, bind by run PID or path, or attach an old run's evidence to a reused request ID. The binding only correlates; it never permits a launch or a share. A published parser keeps its binding after its raw RPC record is dropped.

## Sharing is separate

Three things stay distinct:

1. the descriptive launch evidence above;
2. the Service's decision to share a source, made for a registered factory purpose and an exact source and generation;
3. the authenticated peer's ReadOnly or Write permission.

A V2 attachment must pass the owning Service's share policy and the peer's permission. `SourceKind::ExternalBytes`, `profile.elevation == None`, creation intent, output text and a closed source are not share grants. Elevated and Unknown launches stay local-only under the current policy. A read-only source never gains input or resize rights because its launch was Normal.

The Service keeps evidence in a concrete source-evidence registry. The V2 `Fact`/Unknown representation can report missing evidence internally, and share checks run on the server, so no wire field is needed. The current profile filter is not launch proof. Facts copied into a watch or DTO never stand in for the registry entry when the Service decides to share.

## Limits

This describes the first launch only. It can't promise that later commands, descendants, SSH or container contexts, or elevation the user approves stay non-elevated. Sharing a source is a separate, explicit decision to share its data, and its scope must be stated where it's offered.

## Tests

- Query the test host's own token, then a fixture child the test owns, and compare the observed values. Never hardcode `Normal`. On an elevated host, an ordinary inherited child must not be reported as non-elevated.
- Use only owned fixtures, with no UAC, account or network actions. A mock elevated channel stays `Unknown`. Mapping unit tests don't show native elevation behavior.
- Reject stale or wrong source bindings, missing child proof and token-query failure.
- Published parsers keep their evidence after their raw record is dropped, and a reused request ID can't pick up an old parser or grant.
- Windows and model tests say nothing about Linux or macOS privilege.
