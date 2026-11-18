# Manager-v2 control delta for review

Status: proposed contract, not enabled code. The canonical observer-v2 input gate stays in place. Approved-device Write authority does not create a controller lease. Existing V1 shapes and behavior remain unchanged.

## Device authority versus channel state

The approved device policy is terminal control plus FullAccount filesystem access under the Host OS account's existing ACLs. Approval, revocation, stale approval/view epochs and takeover still apply. Filesystem authority is server-minted, not a roots array or client flag.

A channel can still be unavailable for input because it is intrinsically read-only, awaiting native authentication, missing ownership/recovery support, or subject to the unresolved elevation policy. UI must describe that channel state, not label the approved device ViewOnly. Raw automation's displayed terminal remains intrinsically read-only; its original local stdin API is not widened remotely.

## Negotiated control contract

Use a separately negotiated manager-v2 profile/schema. Do not reinterpret observer-v2 or V1 payloads because method strings are shared.

- Attach/TakeControl asks the original owner to create or take over a real Runtime controller lease after source eligibility, current metadata binding and peer authorization checks.
- The response carries a controller reference bound to connection, approval/view epoch, immutable source generation and actual Runtime control epoch. Delivery authority remains distinct.
- Every input/resize validates that reference under the current owner. Write permission, an observer ID, a source marker or an output ACK cannot substitute for it.
- Review semantic key/paste/mouse input with server-side encoding before enabling the new clients. Do not assume formatted-active-buffer recovery restores every client input mode. Any raw-byte input mode needs its own explicit recovery/encoding contract.
- Preserve captured input intent, canonical input receipts and the shared activity/settlement book. A lost reply never authorizes replay of input.
- Layout authority remains explicit. Observer clipping does not grant PTY resize; an external raw display cannot resize or control its original raw process.

## Native-purpose eligibility delta

Ordinary local catalog sources already have the approved opt-in factory record. Keep their actual local launch privilege check separate.

For review, extend captured native origin, not profile-name inference:

| Channel | Capture point | Activation condition |
| --- | --- | --- |
| SSH client | Trusted Native RuntimeStarter after exact System SSH resource-role checks, bound to connection and Runtime generation | Actual authenticated Ready proof, exact current registry and existing terminal lease authorization |
| WSL/guest channel | Authenticated native guest-owner/transport factory proof | Confirmed guest ownership and exact current generation; unsupported native owner stays unavailable |
| Raw external display | Existing raw owner | No control eligibility; intrinsic read-only view |

Guest privilege remains Unknown unless separately measured. Permission to control an already owned authenticated channel is not a claim that its guest account is non-elevated. The proposed purpose policy must state that distinction explicitly. A local ssh.exe/wsl.exe token never measures guest privilege.

No SSH/WSL eligibility code is activated by this proposal. Do not add a marker-based grant, new default policy or UI control. The admin/elevated boundary remains on hold pending the user's answer. Existing local launch privilege Elevated/Unknown must not be overridden by this proposal.

## Required tests before activation

- Observer-v2 rejects input even for an approved Write/FullAccount device; V1 golden schemas stay unchanged.
- Actual owner Claim/Takeover returns a real control lease; same UUID, stale generation, revoked approval and old control epoch cannot adopt it.
- Lost control/ACK responses preserve custody without fabricated ACK or input replay.
- SSH Preparing waits without guessing identity; exact native identity allows Observe, authenticated Ready permits checked adoption/control, and stale identity rejects.
- Supported guest ownership can activate its captured purpose without falsifying guest privilege; unsupported/unknown ownership remains unavailable.
- Raw remains read-only and exposes no remote AutomationWrite route.
- Current manager metadata authorization runs before exposing IDs; FullAccount file operations preserve validation, byte/type/CAS limits, cancellation, revocation and OS failures.
- Actual normal native captures exercise allow branches; administrative captures exercise denial. No fake Normal, UAC prompt or unrelated process inspection establishes proof.
