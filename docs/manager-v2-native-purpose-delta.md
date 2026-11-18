# Native interactive-purpose delta for manager-v2 review

Status: proposal only. No grant/default/profile activation or admin-boundary change is implemented. Existing observer-v2 local-catalog eligibility query stays unchanged.

## Facts and permission domains

Keep three distinct inputs:

1. Device authority: current approved Actor is server-fixed Write + ApprovedFullAccount. This does not create a controller, native auth readiness or guest privilege fact.
2. Native purpose eligibility: a private owner record for the exact original Runtime generation and policy/auth epoch.
3. Descriptive privilege: actual sealed local launch facts, and independently observed guest facts if any. A local SSH/broker token can never become guest Normal.

Proposed manager-only native factory policy is `FullHostAccountInteractiveChannels`, explicitly installed by native Host composition, not a remote flag or UI choice. It would authorize existing host-owned interactive channels under the rules below. It would not alter the existing `ExistingManagerStandardCatalog` observer policy or reinterpret LocalOnly raw presentation.

## Exact captured purposes and call points

- `Service::prepare_launch_config`/`start_terminal`: retain the original trusted LOCAL catalog route with commandNone. Only actual Started success activates this generation. Current metadata/settings cannot backfill it.
- `ssh_runtime::ServiceStarter::start`: retain the original native SSH manager connection ID and source Identity. Start remains authPending, without effective interactive purpose.
- Existing SSH manager's acceptance of the exact original live authentication proof marks Ready. At that owner transition, capture a private `{identity, nativeConnection, authEpoch}` purpose record. Detach/adopt preserves that original connection/proof, not a prompt, profile string or new ssh process.
- Native SSH revoke, host-key/auth failure, reconnect replacement or original Runtime loss disables the record before queued output/input can proceed. Ordinary remote terminal methods do not approve host keys, supply credentials or install native SSH policy.
- WSL/guest source eligibility requires the actual existing guest process-tree ownership/preflight and native Ready result. Rust WSL is currently unsupported, so no record can activate today. A local wsl.exe/broker token cannot fill the missing guest proof.
- Automation raw starts remain LocalOnly. Its visual observer remains intrinsically read-only; original AutomationOwner stdin/stop ownership is not widened by a device Write policy.

## Proposed effective decision

Local standard catalog source: require exact captured purpose, native policy epoch, actual Started success, sealed local Normal launch evidence and current Actor/profile/generation/control checks.

SSH/guest interactive source: require the exact original host-owned channel, native authenticated Ready proof and current auth/purpose epoch. The explicit proposed channel permission would authorize interaction with that established session while **guest privilege remains Unknown**. The local factory privilege remains its own fact. A known elevated or otherwise held local/guest boundary is not overridden here.

Review must decide this guest-purpose permission explicitly. Do not implement it as `Unknown == Normal`, silently leave it a device ViewOnly role, or grant it from a local token. AuthPending is an inherent channel state, not an approval role.

A proposed private `interactive_purpose_epoch(identity) -> Option<u64>` query could expose only native eligibility/auth epoch to the manager-v2 backend. It would be separate from the unchanged observer-only query and is not approved as a competing current API. Its exact name/type should be published once by the native owner after review. Some would remain insufficient without current Actor, profile, original generation and real Runtime lease checks.

## Real controller association and revocation

Manager-v2 Claim/Take uses the original Runtime only after the native purpose is effective. The server DeliveryRef maps either the real controller lease or a real observer lease. A separate ControlRef/LayoutRef maps only the granted actual controller. No duplicate controller-plus-observer delivery and no observer ACK on controller credit.

Source/auth/policy/approval loss invalidates active refs, queued delivery, recovery IDs and new input admission before bounded detach/retry. Already accepted positive receipts still finish their original activity/CAS bookkeeping; they are not converted into input replay. The source process is not killed merely because a remote view lost authority. A fresh lease/refs plus full stamped recovery is required after takeover or replacement.

Filesystem authority remains the host OS account only. A guest terminal purpose does not reinterpret host-relative file requests as guest filesystem authority or invent a guest cwd. Existing absolute/context, OS ACL, revocation, byte/type and CAS checks remain.

## Tests before activation

- Actual Normal + no native purpose denies; eligible + actual Unknown/Elevated local source denies under the held local boundary.
- SSH Pending denies Claim/input despite device Write. Exact original Ready may activate only the reviewed channel policy; guest privilege stays Unknown.
- Native detach/adopt does not spawn a second SSH session or replace the proof. Revoke/auth-epoch change purges old refs/input before later bytes.
- Guest ownership/preflight failure stays unavailable, including a locally Normal broker.
- Raw presentation stays intrinsic read-only and raw stdin remains with its original owner.
- All controller allow paths use actual owned native evidence where required. Admin inheritance must deny without token overrides while that boundary is held.
