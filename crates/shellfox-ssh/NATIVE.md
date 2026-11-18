# Concrete Service/native SSH adapter

`ssh_native.rs`, `ssh_runtime.rs`, `ssh_tap.rs` and `ssh_open.rs` implement the native adapter rather than another manager/Host/parser. The native owner installs it once through a non-serializable bootstrap task. Neither the remote v1 allowlist nor the local arbitrary-method dispatcher accepts its resource configuration or credentials.

## Published consumer API

- `NativeAction::Ssh(ssh::Request)` returns `NativeResponse::Ssh(ssh::Response)`.
- `NativeAction::SshOpen(ssh::OpenProfile)` returns `NativeResponse::SshOpened(ssh::OpenedProfile)`.
- `OwnerClient::install_ssh_native(NativeSshInstall)` supplies trusted resources over the bounded in-process owner queue. Do not submit it as JSON/RPC.

`NativeSshInstall` contains the already-private `<ProjectUserRoot>/rust-service-v1/ssh` directory, SystemSshAdapter, read-only native imports, askpass program and mandatory `SshProgramPolicy`. That policy must retain and revalidate executable/resource evidence with explicit `SshProgramRole::{Ssh, Keyscan, Askpass}` approval; path-only approval does not authorize a helper role. There is no permissive production default, current-executable inference or acceptance-binary resource selection. Installation rejects another Service's data directory and uses the final Service ownership-loss token.

Native status carries `ConnectionStatus.runtime: Option<RuntimeIdentity>`, populated only by the native adapter after the exact connection/ledger/current-registry check. Preparing without a Runtime returns None. Pending GetStatus/host/prompt replies use the SAME originating HostClient scope as SshOpen. Root must compare a returned readonly observer's actual generation to that expected identity before publishing a view, unobserve a raced/mismatched attachment and retain its own target/epoch cancellation guard. Do not derive a live generation from CoreTab intent, mutable selection, profile tags or Phase. After proof Ready, use explicit AdoptReady and then normal checked Attach.

The owner allocates its monotonic native controller ID and calls `admit_ssh_native`. No UUID hashing or terminal input lease substitutes for that native grant. Direct ungranted calls fail. Native clients can fork views; ordinary detach leaves Ready SSH host-owned. Explicit native capability reduction and app stop retain original cleanup authority until shutdown completes.

CLI/Desktop main use one shared `ssh::dispatch_early_entry` before normal parsing/logging/GUI. Genuine supervisor candidates have no target callback descriptor and retain the original PTY/raw authentication and diagnostics. A callback footprint selects only a candidate, never trust: exactly one arbitrary prompt, including either supervisor-flag string, goes to the authenticated broker. Partial/mixed/extra-argument callback contexts fail silently with code1. Full u32 raw exit bits are preserved. No role, program or production resource is selected by this dispatch.

Interactive key/color/directory dialogs remain in GPUI. A key selection becomes `ssh::SelectedKey::new(path)` followed by ValidateKey. Server replies distinguish actual cryptographic validation from an encrypted/legacy candidate. No chooser/profile RPC family was added.

## One real runtime

`ServiceStarter` implements OwnedRuntimeStarter against the same `Service.host` and `Service.terminals`. It reserves a typed source before the background preparation worker can run. Existing or unclosed runtime/source collisions fail. The ordinary local start path checks that reservation, so attaching during host-key approval cannot auto-start a local shell.

After explicit trust and private broker setup, the starter registers the original generation and calls the existing owned Runtime startup. It does not create a raw SshSession or a second terminal parser. Startup failure retains that runtime identity for cleanup. A missing registry entry alone is never cleanup proof. Source, broker and encrypted identity release only after original closure.

Before a generic terminal Claim/takeover, `Service::check_ssh_control_admission(controller, tab, exact_runtime_identity)` checks the actual registry, source ledger and manager authentication receipt. Preparing/Authenticating, stale identities and closing connections are denied. Readonly Observe does not use this control gate. Once proof establishes Ready, the SSH gate passes for a caller that separately satisfies existing terminal permissions. It neither demands nor transfers the SSH native application grant, and it does not claim input itself. Phase::Active or an ssh-* profile marker is never authentication evidence.

The live tap observes bounded authentication proof from the authoritative parsed live feed, not from view credit, replay, snapshots or a UI success message. It forwards before-input and live output to the ORIGINAL activity sink. Ready therefore needs the approved SSH command's proof, not merely Runtime::Started. The common TTL and captured-settlement input machinery remains installed.

Program inspection uses the actual local executable and SSH/unsupported scope. That does not identify a remote shell, agents or remote process ownership. No default cmd/pwsh profile is inferred for an arbitrary SSH command.

## Creation provenance

`OpenProfile` takes an explicit request ID and uses the Host's durable reserved-publication receipt. It creates ORIGINAL facts as None and never recreates a deleted target or silently restarts an unknown accepted request. Exact target IDs come from that receipt, not mutable Snapshot.selection.

Existing sessions keep genuine historical creation facts. Connect does not reject a session merely because its original intent was local, and no snapshot filter relabels those facts. Current source/process evidence is separate. The remaining v1 projection limit is explicit: its historical shell field cannot fully describe a current arbitrary SSH program; unknown new-program creation intent is not backfilled.

## Lifecycle hooks

Walter's thin native hooks call the real Service adapter methods:

```text
handle_ssh_native(request, controller)
install_ssh_native(config)
admit_ssh_native(controller)
pump_ssh_native()
begin_stop_ssh_native()
stop_ssh_native() -> bool
detach_ssh_native(controller)
revoke_ssh_native(controller)
ensure_ssh_local_start_allowed(tab)
close_ssh_targets(tabs) -> bool
record_ssh_closed(original_identity)
```

Stop/delete hooks retain the metadata and controller on unconfirmed cleanup. Recording closed requires original closure proof. There is no numeric-PID fallback. The external SSH server's process lifetime remains normal SSH PTY hangup semantics, not exact remote PID containment.

## Current acceptance boundary

The actual service lib/test typecheck and the current 20 default SSH tests pass, with two explicit-artifact cases ignored by default. Both artifact cases were run explicitly and passed: the real same-Service Runtime/PTY fixture and the configured private CLI askpass entry fixture. The separate owned native Runtime fixture also passed explicitly on Windows. It uses `Runtime::start_program_with_hooks` with terminal's opaque `TrustedProgramEnv`, not ordinary protected extras. TERM/COLORTERM remain isolation supplied. A finite descriptor carries exactly the six askpass/broker entries to the approved target only, with target/helper resource leases retained through original cleanup; user environment restrictions remain intact.

The configured CLI fixture proves success for BOTH reserved supervisor-flag prompt strings, returning only a synthetic captured answer through the authenticated broker. Wrong capability, partial/mixed callback contexts and extra args fail silently, while normal CLI/supervisor behavior remains unchanged. These cases use a private artifact, not a production-resource selection.

The native fixture holds the actual program behind a proof gate, gets its exact presentation identity from native status, rejects pending generic control/takeover while permitting readonly Observe, and suppresses stale manager/registry identity exposure. Its fake SSH target invokes the explicitly configured role-approved CLI helper with a reserved-flag prompt; only the private synthetic answer allows it to emit proof and reach Ready without view-credit delivery. It accepts Ready control under separate existing terminal permissions, rejects stale/revoked control, retains the common live activity sink, proves a native accepted input clears its correctly captured settlement, preserves None original SSH creation facts, survives view detach/adopts without another Runtime, then confirms original close. This is a Windows owned-fixture result, not a real network/login or cross-platform acceptance claim.

The fixture uses a std-only fake SSH executable and a separately built supervisor under project tmp. It never contacts a real SSH/keyscan endpoint, account, production key or GUI. Native platform, legacy-crypto, prepared-key keyboard-interactive, agent, exit-code and SFTP parity are not implied by this checkpoint.
