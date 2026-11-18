# Native sharing decision for observer-v2

Status: reviewed implementation policy. Native factory/Service integration is in progress; this document does not enable routes or sharing. Missing owner eligibility or actual launch evidence keeps live delivery unavailable. No UI setting or default-sharing change is approved.

## Existing authority

- `src/main/remote/operations.ts` rejects elevated catalog profiles for direct terminal operations, remote AddTab and session deletion. It filters elevated terminal metadata before returning it.
- `src/main/remote/host.ts` requires the existing device approval on pair and every operation. Disposing a view releases delivery, not its process.
- `src/main/terminal/profiles.ts` constructs standard catalog selections and a separate administrator profile. That catalog classification is policy. It is not a measurement of the actual process token.
- Rust `remote_privacy.rs` now follows that per-tab metadata rule. Unauthorized elevated tab identities are excluded before their facts; authorized saved session metadata and unknown creation facts are retained. This grants no live sharing eligibility.

The source owner must capture the sharing decision. A current settings lookup, profile marker, SourceKind, PID, creation intent or `ActualLaunchPrivilege::KnownNormal` alone cannot issue it.

## Proposed private owner record

Keep this in the existing `OwnedTerminal`, bound to its full `Identity`. Do not serialize or persist it:

```
CapturedLaunchOrigin =
  StandardCatalog { selected trusted ShellProfile, local-or-guest domain }
  | RawProgram
  | SshProgram
  | Unknown

ActualLaunchPrivilege = KnownNormal | Elevated | Unknown

OwnerDeliveryPolicy =
  LocalOnly { reason }
  | Eligible { native policy revision, origin }
```

`ActualLaunchPrivilege` starts Unknown unless the actual native launch owner supplies evidence for that generation. A local SSH token says nothing about guest privilege. Neither privilege nor this record proves process exit or descendant cleanup.

The `Eligible` constructor is private to Service's native launch/publication integration. It is not a public boolean provider, RPC field, new remote method or delivery capability.

## Factory and call points

1. Add an explicit native factory option to the existing `RemoteHostSetup`/`RemoteBootOptions` composition: `ExistingManagerStandardCatalog`. It applies the existing manager sharing rule to standard catalog selections. It does not enable HTTP V2 ingress, change default interactive V1 or add a UI control.
2. At `Service::prepare_launch_config`/`start_terminal`, retain the selected trusted native catalog object used for that actual generation. Record candidate policy provenance before starting Runtime; activate eligibility only after the actual Started success. Do not reconstruct this record from saved settings when Attach arrives.
3. At `RemoteDispatcher` owner dispatch, Attach requires the current canonical connection, approval, profile permission and an Eligible owner record for the exact current source generation and policy revision. It then allocates the existing global viewer ID and sends real Observe/ObserverRecoverStamped. No Claim or second parser.
4. Raw `Run.start` records RawProgram + LocalOnly. Successful `owner_automation_show::Viewer::create_read_only` may ask an explicitly approved native Host publication policy for a decision, using the original raw owner and successful reserved publication receipt. Show alone does not grant sharing. Raw publication remains LocalOnly under the proposed initial factory setting; remote raw parity stays blocked pending a separately approved publication policy.
5. `ssh_runtime::ServiceStarter::start` records SshProgram + LocalOnly. A standard local ssh executable or local token cannot authorize unknown guest output. No SSH/guest eligibility is added here.

A source already launched before this capture existed stays Unknown/LocalOnly. Do not backfill it from the current catalog, first tab or creation facts.

## Approved delivery decision

An exact trusted **local standard catalog selection** may receive a pending eligibility record under the explicit `ExistingManagerStandardCatalog` factory policy. Capture the original selection at launch and activate the record only after actual Started success. Do not backfill it from current settings or metadata.

Eligibility alone is not effective delivery permission. Attach additionally requires the exact generation's sealed `LaunchPrivilege::Normal` observation, current source-policy revision, authenticated connection and current peer permissions. Elevated or Unknown veto live delivery, including a non-elevated token with conflicting high-integrity/UIAccess evidence. Raw, SSH and guest sources stay LocalOnly unless a separate native publication policy is reviewed.

There is no catalog-intent bypass for missing measurements. Unsupported platforms remain unavailable rather than becoming Normal. Metadata visibility is checked separately before returning unauthorized tab IDs. Saved session metadata remains under its existing manager permission, including an all-elevated session with an empty authorized tab list. Revocation invalidates queued delivery locally before bounded Runtime detach is attempted.

## Revocation

- Device revoke/remote disable uses the existing ApprovalControl epoch and invalidates connection tickets. Cleanup sends only Unobserve and clears that view's mailboxes.
- A native source-policy revision change runs on the same Service owner. Revoke affected source delivery before further data, emit canonical Revoked, and retire the exact observer lease. No raw stop or metadata deletion follows from delivery revocation.
- Store the source policy revision with the internal Attached record. Poll, ACK and recovery recheck it with full identity. An old wire recovery UUID cannot renew a revoked grant.
- Generation replacement starts with a new capture. Owner/backend replacement invalidates old tickets and retires their observer leases. Saved records cannot resurrect a source grant.

## Required decision tests

| Capture and policy | Expected delivery |
| --- | --- |
| KnownNormal, no Eligible owner decision | Unavailable |
| Standard local catalog capture, explicit standard factory policy, Started confirmed, privilege Unknown | Pending eligibility may exist, but live delivery is unavailable |
| Same capture with sealed KnownNormal and current source/peer permissions | Delivery may attach under the exact current policy revision |
| Elevated actual source, any catalog label | LocalOnly |
| Unknown launch origin or only current settings/profile marker | Unavailable |
| Raw Run before/after Show, no approved raw publication policy | LocalOnly |
| Host publication fails | No grant or metadata activity binding |
| SSH local token KnownNormal, guest privilege unknown | LocalOnly |
| Stale generation, source-policy revision, approval or connection | Rejected/revoked before data |
| Detach/revoke | Observer released; original process remains owned |

Existing tests already cover real observer recovery/ACK custody, opaque same-sequence recovery rejection, actual geometry barriers, current approval and connection replacement, and unknown metadata coverage. They do not establish the proposed sharing eligibility or native privilege observations.
