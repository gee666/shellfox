# Headless owner to GPUI native attachment

Status: architecture direction approved within the existing one-Service model. A callable bridge or broker is NOT approved until actual resource trust and each OS launch policy are established. No implementation, new wire methods or dependencies accompany this document. This is separate from the observer-v2 remote/mobile protocol proposal.

## Approved direction and remaining gates

Keep the original headless process as the only Service/metadata/runtime owner. Add an owner-gated trusted UI bootstrap and a separately attested native UI connection to that same owner. GPUI receives immutable frames from the existing parser. It does not open SQLite, construct another Service, allocate replacement PTYs or reconstruct a second desktop parser from ANSI replay.

Retain the current in-process path for an embedded desktop owner. Keep one HostClient presentation API with two real backends: in-process native access and authenticated attached native access. Add the attached backend only alongside its working transport and tested DTO projections, not as a declaration-only interface.

An independent launcher does not acquire native UI authority by requesting a desktop launch. The current existing-owner handoff rejects before Submit when no desktop is registered. Future bounded pending-intent admission and broker activation require a separately enabled capability.

## Current source gaps

- `shellfox-service/src/native.rs`: NativeClient wraps OwnerClient. NativeAction and NativeResponse are in-process Rust types, including Arc-backed frames and host-only capabilities. They are not a wire codec.
- `owner_client.rs` and `owner_native.rs`: admission is bounded and an admitted mutation is never retried merely because its wait disappears. Preserve these semantics across IPC.
- `owner_private.rs` and `owner_private_client.rs`: private Discover/Submit/Receipt and trusted native registration/claim/completion are deliberately separate task variants. Keep this boundary.
- `shellfox-desktop/src/host_client_endpoint.rs`: HostEndpoint starts embedded local serving and obtains NativeClient through the readiness callback. It has no external-native attach transport.
- `root_startup.rs` and `app_host_backend.rs`: Root installs the embedded EndpointBackend. The current existing-owner launch handoff is launch-only, not a complete native UI attachment backend.
- `app_host_close.rs`: the final-window path calls backend Stop. Attached windows need a different application-close policy.
- `host_client.rs`, `host_client_frames.rs` and service `native_cache.rs`: native frame acceptance depends on bounded immutable storage, identity/version checks and an explicit AcceptFrame operation. Pointer identity checks cannot simply cross a wire.
- `files.rs`: the current GPUI file panel constructs FileService::local_desktop directly. It is not an owner-backed attached file client.
- `root_startup.rs`: profile discovery currently runs in the GUI. Attached profiles must come from the original owner.
- `settings_api.rs`: an auxiliary SettingsServices interface exists, but no production implementation was found in the reviewed desktop wiring. A transport alone will not implement those settings pages.
- `experiment.rs`: the private local protocol can carry full global SettingsRequest, metadata and terminal byte operations, but lacks native ViewFrame, observer presentation and versioned selection coverage. Its declared ANSI recovery is VisibleScreen, not full native presentation.
- The shared remote 43-method API intentionally excludes local settings administration, remote login/approval management and other trusted host capabilities. Do not widen it for this task.

## Two frontend approaches

### Extend the ordinary shared local frontend

Useful existing pieces are typed local requests, private endpoint verification, byte limits, deadlines and cancellation. Continue using them for CLI operations and launch Discover/Submit/Receipt.

Using this path for the full desktop would still require native frame projections, observer/presentation operations, host administration and stronger caller attestation. Putting those behind the existing shared endpoint credential would incorrectly make every general private client a trusted UI.

If separate native attestation and role-bound dispatch are added, this option becomes the native connection below. Reusing serializer/framer code does not remove the authorization boundary.

### Separately attested native connection, preferred

Use a distinct native UI admission scope and connection lifecycle, while forwarding into the existing OwnerClient and async host managers. Reuse low-level private stream/framing code where safe. Do not reuse the general endpoint credential as proof of the native UI role.

The reviewed private endpoint proves protected endpoint access and same SID/UID, plus a shared credential. That is appropriate for its current contract, not executable/launch attestation.

## Trusted bootstrap and independent launch sequence

This is the proposed sequence, not an added RPC or CLI contract.

1. The independent launcher validates its ordinary launch intent and discovers the original owner. The current pinned handoff uses Submit/Receipt only after confirming an existing registered desktop. It does not open Root, start HostEndpoint or claim that its own executable is trusted.
2. Without a registered UI, the current `local_launch.rs` path rejects BEFORE Submit. It does not queue an independent request. No launch receipt is marked handled, no session is inferred from the latest snapshot, and no metadata is created merely to pretend a desktop exists.
3. A future, separately enabled startup capability may admit a bounded pending intent and activate an owner-side broker only after trusted package resources, compatible native attachment support and an already-authorized interactive user session have been verified. Queue admission and broker activation are future behavior, not the existing handoff. Until those gates are satisfied, preserve rejection before Submit.
4. The owner chooses the exact desktop executable and fixed arguments from its trusted package resource provider. No executable, environment override, PID or bootstrap credential supplied by the independent caller selects this program.
5. The owner creates one pending bootstrap attempt, bound to its current owner fence and a one-use private channel. It starts the verified UI and retains its actual launch/lifetime receipt. Concurrent intents share this bounded single-flight startup attempt; they do not each spawn a GUI.
6. The bootstrap channel is inherited or transferred only to that verified child. Never put its authority in the general endpoint record, ordinary argv or a reusable shared token. Child descriptors/handles are closed to later descendants.
7. Both sides verify their actual OS peer/process incarnation and expected package role, then challenge the one-use channel and bind the connection to the original owner fence. A claimed process path or JSON PID is not evidence.
8. Only after successful attestation does the owner create a native app session. That app session may register once for launch delivery and fork server-bound window clients. The requester still has only its normal launch receipt.
9. GPUI uses that session to load its real workspace before claiming and applying queued UI intent. Claim, application and completion retain the existing coordinator's no-replay behavior.

The approved initial policy is owner-launched verified child only, in an already-authorized interactive user session. No display/session/credential guessing or service-to-user-session escalation is allowed. An independently running GUI must not gain authority by presenting the private token or claiming it is the configured program. Direct attachment of an already-running GUI is outside this approval. An independent process can remain a launch requester; future handoff to a newly verified child requires the separately enabled startup capability.

No new startup-consent field is silently added to Georgia's current DTO. If product policy requires consent beyond its existing explicit desktop intent, specify and approve a separate versioned request before implementation.

## Required trust root

SystemInstallResources already expresses the right composition boundary: a configured package provider backed by a validated package lease. Its current structural checks and validate_current file-existence checks are not publisher authentication and cannot manufacture that lease.

Reuse `shellfox-resources` external manifest anchors, VerifiedBundle and its verified executable/child lifetime machinery. Do not invent a second trust system or derive a trust anchor from the installed payload. The next feasibility review must establish whether those existing contracts can bind both host and desktop roles, compatible attachment versions, executable/dependency identity, bootstrap inheritance and OS peer/process incarnation. Missing resource roles or per-OS launch guarantees are gates to review, not permission to make the bridge callable.

Forbidden substitutes are current_exe(), runtime self-hash, executable filename alone, an acceptance binary, the Go helper's expected digest, a payload PID, and the existing shared local secret.

Retain a race-resistant process incarnation and executable identity across launch and handshake. A path checked before launch but reopened after replacement is insufficient. Protect bootstrap inheritance and the relevant loader environment too. The policy must state its limits against same-user process injection/debugging; attestation is not a claim of isolation from a compromised trusted process.

### Resource feasibility review

The read-only review confirmed reusable verification machinery, but no complete frontend launch provider exists yet:

- Compiled manifest anchors and the closed bundle schema do not currently define host/GPUI attachment roles.
- `VerifiedBundle` retains verified objects; that alone does not authenticate the running owner or authorize an interactive session.
- `VerifiedCommand` exposes mutable process configuration. It is not a constrained GUI bootstrap/inheritance/loader policy.
- `VerifiedChild` protects the bundle through direct-child exit, not descendant cleanup or mutual process-incarnation attestation.
- Unix immutable execution remains explicitly unavailable. Windows still needs original-process, inheritance, namespace/loader and interactive-desktop proofs; GPUI GPU dependencies cannot inherit a helper's closed-loader approval.

The next prerequisite is a concrete approved package-role, interactive-session and attested child-launch provider using existing resource contracts where valid. Missing evidence remains unavailable; no helper role, runtime self-hash, generic opener or shared credential substitutes for it. Browser/file-manager delegation must remain outside the GUI cleanup domain. No broker is enabled by this review.

## Transport and authority scope

The native channel is private OS IPC, not TCP or tailnet. Its authorization context is created by bootstrap, not deserialized from a request. It binds owner fence, connection incarnation, app grant and server-created window clients.

The server maps each window to its existing OwnerClient fork. Caller-supplied controller, generation, epoch or frame identifiers remain untrusted and must match that binding. Registration/claim/completion are dispatched only within the attested app scope. They are not variants of the general private namespace and are not remote/mobile methods.

After the attested frontend reports readiness, the bridge invokes the existing native desktop_register locally and retains its unique DesktopRegistration receiver. It does not create another launch router. Delivery polling/streaming is bounded by that receiver. Claim and completion resolve a server-retained delivery reference and recheck the existing owner/client/receipt; a caller cannot manufacture a DesktopLaunchDelivery or choose another registration. Disconnect unregisters that actual native registration. An attested readiness report is still not a launch Handled receipt; only the normal claim/apply/completion sequence can establish that result.

Use a closed, separately versioned native request/response projection. Start from the operations actually used by HostClient. Validate all external input and response fields; do not derive serde blindly over internal NativeAction or anyhow::Error. Return typed admission/validation/uncertain outcomes without leaking credentials, argv or environment in diagnostics.

Keep the existing 16-entry owner admission bound, HostClient request budgets, two frame cycles and bounded view/cache limits. Add bounded connection, response-retention and frame-assembly budgets. Work waiting on IPC or serializing an immutable frame must not block Service::pump or become another Runtime::drain consumer.

Reuse the current 1 MiB/8 MiB general framing limits where applicable. Native frames can exceed an 8 MiB encoded response under existing native cache budgets. Use bounded chunks and a bounded immutable assembly instead of silently increasing shared/mobile limits. A partial frame is never publishable or credit-bearing.

## Terminal and immutable frame projection

Transmit validated values, not pointers, handles, Arc layouts or Ghostty binary snapshots. Preserve identity, generation, controller/observer lease, frame ID, presentation version, output sequence, recovery barrier, exit boundary, row metadata, cursor, selection and history coordinates.

Reconstruct one immutable screen allocation on the frontend and bind the decoded presentation to it. Validate cell/grid products, row metadata length, text budgets, sequence/version monotonicity and the grant for that exact view before replacing the latest-frame cache.

The owner continues to encode Key/Paste/Mouse using its actual terminal modes. Freeze input settlement and user-input metadata at frontend FIFO admission; do not reconstruct it at dispatch after state changes. Do not invent a string route for arbitrary binary mouse bytes.

An observer grant does not imply canonical layout authority. Current ResizeExternal changes the canonical parser grid. Initial attached observers should clip the published canonical grid and must not automatically map a window resize to ResizeExternal or PTY resize. Any explicit canonical-layout authority needs separate policy and tests.

Keep three receipts distinct:

- Transport response consumption keeps shutdown draining bounded; it grants no terminal credit.
- AcceptFrame grants credit only after the complete, validated immutable frame has entered the frontend's bounded cache. Preserve generation, offered-frame and recovery-barrier checks. Receiving/enqueueing chunks is not consumption. Painting need not be the boundary.
- Launch completion records the result of one claimed UI effect. Neither receipt above can substitute for it.

## Full settings, files, profiles and SSH

An attached desktop is not complete when only launch delivery or a terminal demo works.

- Metadata and global settings use the existing owner paths and full Settings DTO/patch omission semantics. Do not replace them with ManagerSnapshot v1 or default Preferences.
- Profile/catalog discovery and settings validation run on the original owner. GUI-local discovery cannot determine its launch availability, elevation or process facts.
- Remote configuration, login, approvals, reconnect and revocation route through the existing async remote manager. Do not block the owner waiting for network work, expose auth keys in Debug, or create another remote manager for the GUI.
- SSH list/save/delete/import and host credential/key validation remain owner-managed. Frontend pickers may stay GUI-native, but their results are validated data, not execution authority.
- Introduce a real Files backend boundary for the GPUI panel. Attached file operations reach the original owner's existing file service/worker and preserve revision conflicts, streaming bounds, path policies and commit checks. LocalDesktop filesystem authority is constructed from the trusted app role, never accepted as a payload flag. Do not leave attached mode silently using GUI-local FileService access.
- Directory completion and host-side paths use owner facts and policies. Window dialogs, color pickers, clipboard and activation can remain frontend-native where appropriate, without becoming arbitrary host shell commands.
- Complete SettingsServices auxiliary wiring and test each supported feature. Unsupported platform features remain honestly unavailable; an empty implementation or a fake successful response does not satisfy full UI coverage.

None of this expands the remote 43-method allowlist. Raw-process lifecycle and original-process cleanup remain with their existing authoritative owners, not source tags or GUI registration.

## AppHost lifecycle and ownership

Represent owner scope explicitly rather than inferring it from window count:

- EmbeddedOwner retains HostEndpoint and its existing confirmed Stop, owner-thread completion and cleanup report.
- AttachedFrontend retains only the attested app connection and per-window leases. Application close drains/guards edits, detaches windows and unregisters the UI session. It does not invoke Stop on the original headless owner.

Do not report an invented zero terminal count to bypass close behavior. Preserve actual counts; change the policy by scope. Dirty file/settings guards still apply. An explicit Stop Host operation remains separate from ordinary window/application close and uses the original owner's confirmed cleanup path.

AppHost must distinguish frontend-detached completion from host-stopped completion. An attached GUI may exit after confirmed release even while the headless owner is healthy. Conversely a dead connection is not proof that the owner stopped or that host cleanup completed.

Do not transfer HostOwnership, metadata locks, source consumers or PTY guardians to the GUI. Original owner exit/loss still invalidates every connection and follows current resource cleanup. This proposal adds no shell-survival or owner-migration promise.

## Cancellation, uncertain effects and loss

Before admission, Full/Stopped/validation failure means no command was enqueued. After admission, dropping a wait or canceling transport must not resubmit a mutation. Retain bounded operation state long enough to distinguish an accepted command from a new intent; uncertain outcomes remain uncertain.

A canceled bootstrap wait does not launch a second GUI. Retain the same startup attempt and child receipt until ready or confirmed failure/cleanup. Startup failure must never be reported as launch Handled. Terminate only a provably owned failed GUI child when needed, never host terminals or a guessed PID.

For launch delivery, keep the existing owner recheck, claim, one UI application and completion phases. If UI effects may have happened but completion is lost, do not replay focus/window creation. Resume completion where its result is known; otherwise preserve an uncertain receipt.

Connection loss releases that app's delivery receiver, observers, controller leases and frame caches, not the original Service. Reconnect establishes fresh attestation/grants and frame recovery. It does not replay keyboard input, recreate sessions or infer the last selected session from metadata. Owner loss invalidates registration and pending grants before any further UI effect.

## Platform prerequisites

Windows: private pipe DACL and same SID remain necessary. Add exact peer-process incarnation and expected host/UI package-role verification. Retain real launch handles, restrict handle inheritance and validate the executable/loading policy without path-replacement races. The owner must already be in an authorised interactive user session. A Session 0 service must return unavailable; this approval does not permit acquiring another user's session, credentials or desktop.

Linux: private Unix sockets and peer credentials remain necessary. Add durable process-incarnation evidence, protected executable/package verification and restricted inherited descriptors. The owner must already have an authorised interactive session. It cannot guess DISPLAY, Wayland or user-session bus authority from caller strings. If that prerequisite is absent, startup is unavailable; session acquisition/delegation is outside this approval.

macOS: private local streams and user identity remain necessary. Add audit-token/process-incarnation and designated code/package-role verification with the supported deployment targets. A daemon/SSH context is not an Aqua login session. The owner must already have authorised interactive-session access; introducing user LaunchAgent/session delegation is outside this approval.

Android/iOS retain outbound-only mobile clients. They do not acquire this local native UI role. The headless service and IPC definitions must not depend on GPUI, GPU initialization or a display server.

No concrete OS attestation API availability or packaged trust provider is claimed verified by this design review. Missing platform prerequisites leave UI startup unavailable; they do not authorize a shared-secret fallback.

## Staging and acceptance

1. Agree trust policy, package-role evidence, bootstrap ownership and close semantics. Unit-test rejection and lifecycle models without introducing callable placeholder facades.
2. Implement one complete attested attachment slice: same owner, native status/settings and one terminal frame/input/consumption cycle. Test real framed transport, cancellation, bounds and loss. Do not label this full desktop support.
3. Wire the full HostClient/SettingsServices/files/profile/SSH coverage and launch coordinator. Prove the independent requester never receives native trust authority.
4. Run native Windows, Linux and macOS acceptance with real GPUI and existing headless owners. Only then enable the startup broker in supported composition.

Required tests include wrong package/image/peer/incarnation, replaced endpoint/executable, leaked or replayed bootstrap, stale owner/client grants, cross-window lease reuse, general-private-token attempts at native trust operations, fragmented/oversized frames, forged ACKs, slow clients, settings omission/validation, file conflicts, SSH secret redaction and remote-manager cancellation.

Count actual Service/Host/Runtime/parser constructions to prove single ownership. Close/crash the test GUI and prove original headless PTYs remain; explicitly stop the owner and prove cleanup/invalidation. Test queued launches before registration, concurrent startup intents, canceled startup, lost claim/completion replies and no duplicated UI effects. Exercise dirty/pending multi-window close and distinguish GUI-detached from owner-stopped proof.

## Next feasibility review

The direction is approved, not a callable bridge/broker. Review the actual `shellfox-resources` anchors/VerifiedBundle contracts and per-OS support for launching and attesting an owner-launched verified child in an already-authorized interactive user session. Identify missing evidence and obtain approval before implementation. No display/session/credential guessing, service-to-user-session escalation, independent-GUI attachment or self-trust fallback is included.

Until those prerequisites are satisfied and the startup capability is explicitly enabled, preserve the current unregistered-owner rejection before Submit. No new wire method, namespace, endpoint or trust verb is registered by this proposal.
