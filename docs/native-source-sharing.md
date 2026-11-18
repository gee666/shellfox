# Native source eligibility policy

This private owner API supplies source-policy eligibility. It does not supply launch privilege, peer permission, layout authority or process-cleanup proof.

## Existing policy and concrete callpoints

The existing app checks approved remote peers and current commit permission in `src/main/remote/host.ts`. `remote/operations.ts` excludes administrator profiles and permits standard terminals under that authorization. Rust keeps those checks in `Actor::with_authorized`, `policy::authorize` and Service's existing metadata/profile policy. There was no independent per-generation source eligibility record.

The Rust owner now captures the trusted local catalog route in `Service::start_terminal`, after `prepare_launch_config` and before moving config into Runtime. `command` must be absent and the selected local catalog profile must be present. WSL is excluded. Generic/program constructors, raw automation and SSH use `OwnedTerminal::new` and start LocalOnly. Neither `automation-pipe` nor a later profile/settings value can create captured catalog origin.

`OwnedTerminal.startup` remains the one startup result. The sole `Service::drain` activates eligibility only when it receives the first actual successful Started result. Phase alone does not activate it.

## Explicit owner configuration

`NativeFactoryPolicy` has two values:

- `Disabled`, the default.
- `ExistingManagerStandardCatalog`, an explicit native opt-in to the existing standard-catalog policy.

`RemoteBootOptions::with_observer_factory_policy` carries that native configuration. The retained boot guard installs an opted-in policy through the same owner queue before endpoint readiness. This is not a wire option, new UI switch or default ingress enable.

An explicit later installation may activate an already successful generation using its original captured origin and stored Started result. It never queries the current catalog or reconstructs origin from saved metadata.

## Backend contract

`Service::observer_delivery_policy(identity) -> Option<u64>` is the one backend query. It checks exact current Runtime identity/generation, captured local catalog origin, successful Started, current policy and matching policy revision. None means there is no active source-policy eligibility. The number is only a policy epoch.

Some is insufficient for delivery. V2 must also check:

1. Actual sealed `Runtime.launch_privilege_facts()` is explicitly KnownNormal with the correct factory purpose and capture stage. Elevated and Unknown veto delivery, including administrator-token inheritance.
2. Current metadata binding and existing manager visibility permission permit disclosure.
3. Current authenticated peer/approval/view permission permits observer delivery.

A normal token without a source record is denied. An eligible catalog record with unknown/elevated privilege is denied. An approved peer cannot turn a LocalOnly raw/SSH/guest source into an eligible catalog source. Metadata authorization remains separate and must run before exposing IDs.

## Revocation and custody

Policy changes first set Disabled and clear active generation records. They then call the real `retire_remote_observers(identity)` hook, which invalidates delivery/recovery tokens and purges queued data before bounded Runtime Unobserve. Full retains retry custody; it does not leave queued bytes readable. Only afterward may a new policy revision activate captured successful sources.

The backend stores/rechecks the epoch before offer, ACK and commit. Source replacement and shutdown invalidate the old eligibility. Observer retirement never kills the source or fabricates an ACK.

## Tests

Policy unit tests cover captured-origin immutability, exclusion of guest/program routes, missing Started, mismatched identity and default Disabled. Owned Windows tests cover actual Started activation, explicit installation after captured birth, failed startup, Raw LocalOnly, and revision revocation/reinstallation. These tests make eligibility assertions only.

End-to-end allow tests must use a real owned capture and run the allow branch only when that capture is Normal. Administrative hosts must exercise the Elevated denial branch. Unsupported captures remain Unknown; no fake elevated/alternate-token fixture or override establishes native proof.
