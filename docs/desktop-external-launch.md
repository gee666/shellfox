# External desktop launch helpers

Current production source uses retained prestart in `app_startup.rs`, the Service's authoritative handoff loop and the app-scoped callback binding in `launch_runtime.rs`. This source wiring is not native Explorer/CLI acceptance evidence. Walter owns local/owner/CLI routing. Eva owns Options, main and Root binding. Salahuddin supplies stable packaged launch resources through the existing system adapter. The unused desktop planner facade and its test-only models have been removed; they are not an alternative startup path. This task changes only owned launch helpers, focused tests/docs and the previously approved module declarations. Existing AppHost and WindowSelection implementations stay unchanged.

## Source behavior

Read against this worktree's Electron `src/main/cli.ts`, `index.ts`, `attached-desktop.ts`, `platform/shellfox-cli-launch.ts`, `platform/explorer.ts`, `external-request.test.ts` and current Rust creation/local-client flows.

- `start [path]` and its `add`, `new`, `create` aliases resolve against the invoking process cwd. No path means that cwd. Directory validation happens before forwarding.
- Desktop `--new-session --cwd ABSOLUTE` is a paired contract used by Explorer. Duplicate/mixed flags, surplus paths and malformed directories are rejected.
- Electron's single-instance handler queues an explicit request until the renderer is ready. The attached desktop calls the existing host, not another SQLite/PTY owner.
- An explicit user request creates one session with its default-profile tab, selects it and presents one window. Programmatic session creation is not a request to focus every desktop.
- A new invocation for the same path gets a new request ID. Retransmission preserves the original ID.

The Rust helpers keep creation as Service-owned metadata. Only normal Root terminal attachment can start the one existing owner's PTY. The launch helpers expose no PTY, takeover, observer or process-control API.

## Product and internal flags

`app_launch::parse_launch_args(args, invoking_cwd, directories)` accepts product arguments after existing CLI/Options globals have been separated:

- No launch arguments means `Show {}`.
- `start|add|new|create [path]` means `NewSession { cwd }` after invoking-cwd resolution and native directory-policy validation.
- `--new-session --cwd ABSOLUTE` is the Explorer equivalent.
- `--new-window` is an explicit window disposition, not inferred from metadata creation.
- Internal `--launch-request-id UUID` carries one ID through a configured cold launch. Installers must never pin an ID in a launcher/menu command.

Options globals `--attach-existing-host --launch-owner FENCE` mean launch-only handoff to that exact already registered desktop, then secondary-process exit only after the matching `Handled` receipt. Eva's wrapper/parsed flag path delegates to the Service's pinned helper. Ordinary startup uses the actual retained prestart result: Ready retains that same AppHost for the graphical app, Occupied uses the existing-owner handoff and Failed retains cleanup until confirmation. There is no desktop planning/probe model, rendering attachment, extra launcher or cold fallback introduced by these helpers.

Windows drive/WSL and POSIX lexical rules are kept separate, including spaces, dot segments, root-relative paths and same-drive relative paths. Other-drive relative paths are rejected because resolving them would require ambient per-drive cwd state. No tilde/account expansion occurs. Both the invoking and owning native system adapters validate directory existence/access and local-drive/WSL policy through `LaunchDirectoryPolicy`; lexical validation is not an existence or ownership proof.

## Exact private namespace

The shared opt-in protocol constant is `shellfox_protocol::DESKTOP_LAUNCH_NAMESPACE`, aliased by the service as `DESKTOP_LAUNCH_METHOD`. Its only value is `experimental.desktop.launch.v1`. Do not accept an `experimental.*` prefix.

The service owns `DesktopLaunchRequest` with exactly these commands:

```rust
Discover {}
Submit { owner, intent, submission: LaunchRequestKind::Fresh | LaunchRequestKind::Known }
Receipt { owner, request_id }
```

The JSON tags are `discover`, `submit`, `receipt`. `DesktopLaunchIntent` contains `requestId`, an action and explicit window disposition. Empty `Discover {}` and `Show {}` variants reject extra fields. Registration, poll, claim, completion/ACK and stop are not private request DTO operations. Keep those on the trusted authenticated native desktop path, bound to the caller's actual client identity. No public/mobile allowlist or remote permission changed. The compile-fail contract test proves this DTO does not implement the sealed remote `Request` trait.

The local server must validate the namespace, typed payload and actual ownership before dispatch. `handle_local` does not create a session or show a window; Submit requires a registered desktop and only admits a bounded intent. Headless Submit fails with `NoDesktop` before admission. Registration/poll/claim/ACK stay in-process trusted native APIs. There is no `DesktopLaunchClient` transport or second handoff loop in the desktop helper.

## Service owner API

Keep one `DesktopLaunchRouter` beside the existing Service on its owner thread. Do not construct it per command, window or renderer. The separate router placement also avoids borrowing a router field and the whole Service mutably at once.

```rust
let owner = router.owner();
let registration = router.register(owner, &service)?;
let mut resolver = ServiceLaunchResolver::new(&mut service, &native_directories);
router.dispatch_one(owner, &mut resolver)?;
```

Exact methods:

- `register(owner, &LaunchAuthority) -> Result<DesktopRegistration, LaunchError>` returns one token and one non-cloneable receiver. A live registration rejects another coordinator with `Busy`.
- `poll(&mut DesktopRegistration, &LaunchAuthority) -> Result<Option<DesktopLaunchDelivery>, LaunchError>` reads at most one delivery and rechecks ownership/client fencing. Raw `registration.poll_delivery()` reads the channel only; it is not authorization.
- `enqueue(owner, intent, &authority)` admits a fresh request. `resume(owner, intent, &authority)` recovers a known ID. Both return `LaunchReceipt`.
- `dispatch_one(owner, &mut LaunchSessionResolver)` reserves channel capacity before metadata resolution and sends at most one typed delivery.
- `claim(&delivery, &mut resolver) -> Result<LaunchClaim, LaunchError>` rechecks actual ownership, current registration, original intent and durable target immediately before UI effects.
- `complete(&delivery, LaunchHandling, &authority) -> Result<LaunchReceipt, LaunchError>` records the result. Repeating the same ACK is idempotent.
- `unregister(token, &authority)` retires only that client. It does not stop Service/PTYs. Ambiguous dispatched/applying requests become `Uncertain`, not a replay to a different client.
- `invalidate_owner()` fences queued/delivered work and closes the sender. Thread completion, endpoint-file absence or a numeric PID is not an ownership proof.

The authority implementation for Service uses its real ownership check. The router has a fresh owner fence per lifetime and a fresh client token per registration. A queued message surviving owner/client loss must still fail claim before any window effect.

Bounds are 64 pending intents, 32 channel deliveries and 1024 retained request receipts. Receipt exhaustion rejects new IDs instead of evicting old dedup evidence. Backpressure/no desktop does not create metadata. A lost UI consumer never causes automatic focus/new-window replay. Status receipts describe the recorded request outcome, not current session liveness.

## Durable creation receipt

`ServiceLaunchResolver` calls the actual HOST-only API:

```rust
host.creation_request_receipt(request_id)
    -> Result<CreationRequestReceipt>
```

`SessionCreated { session_id }` proves only the original session. The resolver returns `LaunchTarget::Session { session: session_id, tab: None }`. It never reads `Snapshot.selection`, assumes an original tab survived, claims a runtime is alive or reconstructs creation settings.

For `Known` submission, missing/deleted receipt means `ReceiptUnavailable`. No recreation, fallback selection, inferred tab or backfill is allowed. A known UI-only Show/OpenSession request with no router receipt is `DeliveryUncertain`, not replay permission. Callers must never relabel an uncertain/known retry as Fresh. Fresh means a genuinely new user invocation with a newly generated ID. On initial creation only, the existing Service chooses the trusted default catalog profile and captures its creation facts. The directory basename/root title follows the old external launch policy.

The same durable lookup is checked again at claim. A session deleted after dispatch cannot be silently replaced or focused via stale target data. Across owner restarts the durable session mapping, not the global recent selection, resolves known creation IDs. In-memory payload conflicts are rejected; the durable receipt is not a stored hash of the original argv.

## App coordinator and production binding

Keep one non-cloneable `AppLaunchCoordinator` at app scope, independent of Roots:

- The owning UI uses `new(DesktopRegistration)` with an actual in-process receiver.
- `pump(&LaunchDeliveryClient, &mut LaunchWindowTransport)` polls at most one channel event. Its native client methods are `claim` and `complete`.
- There is no rendering constructor from a cross-process owner/client token. Native registration, poll, claim and ACK are not added to the general private wire. Headless-owner to rendering-GUI attachment remains an explicit full-migration blocker pending a separate transport design review.

Eva's `launch_runtime.rs` now implements and binds `LaunchWindowTransport::apply(request_id, &LaunchTarget, LaunchDisposition)`. That existing production callback was inspected but not edited by the cleanup task. It must select/present one eligible local Root, or use the existing AppHost window allocator and create one new native window with a local `InitialSelection`. Existing windows' active tabs must not follow `Snapshot.selection`. Show restores/presents one window without creating metadata. New-window actions fork the existing HostClient through AppHost, not another endpoint. Terminal access remains ordinary non-takeover attachment, including the real ControlRequired overlay.

Register the app coordinator before handling launch-created workspace windows or terminal views. Bootstrap/error UI keeps its existing startup policy. A second UI registration must forward/exit on `Busy`, not start another Host or create speculative PTYs. Preserve source icon/title, accessibility context, bounds, dirty-file guards and confirmed app shutdown behavior. These helpers do not close other windows or touch any process.

The coordinator retains its phase across cancellation. A cancelled ACK wait resumes only ACK, never window apply. A cancelled native apply is uncertain and is never run a second time. Malformed ACK responses do not repeat already applied focus. Keep the app coordinator/pump alive when an individual view disappears; do not bind delivery lifetime to a weak Root task.

## Configured resources and the single handoff policy

The authoritative production handoff is Service `local_launch.rs`, currently `LocalClient::launch_existing_pinned(intent, submission, expected_owner)`: one Discover, pinned-owner check before Submit, request-ID correlation on Submit and every Receipt response, then wait for `Handled`. Eva's `host_client::handoff_existing(config, intent, submission, owner)` delegates to it without a second loop. Queued/Dispatched/Applying are pending, never successful handoff. Receipt polling keeps the same owner and never rediscovers or resubmits. Eva owns the pinned desktop wrapper and flag binding. This task does not modify or duplicate that loop.

No second desktop discovery/Submit policy or unused future planner remains. Production prestart owns its real endpoint before graphics and retains it through initialization failure; an occupied-owner result delegates to the existing Service helper. Auth/ownership/transport uncertainty cannot become an unproven no-owner fallback. A host without a desktop fails clearly through the authoritative helper/private handler; it cannot create a rendering NativeClient, launch another UI or start another Host.

Stable executable/prefix and installation-resource policy remain in the existing system adapter and package composition. The removed planner's mock resources were not a production consumer. No acceptance/build EXE, registry/PATH mutation, shell-launch policy or account/network action was added. OpenSession remains a typed action to the registered desktop, not a replacement-session command.

A queued Submit is not proof of PTY startup or successful window presentation. Preserve the request ID after any uncertain submission and query its Receipt; do not automatically relabel a Known retry as Fresh or issue a replacement ID. Only a correlated `Handled` receipt from the authoritative Service loop authorizes successful secondary-process exit, never Queued admission alone. Neither authorizes a rendering client.

## Verification and open work

The active helpers use injected directory, host/session, native client and window interfaces. No registry/PATH/install/open/window/user-process action is exercised. Service tests cover admission, bounds/backpressure, duplicate/conflicting IDs, restart targeting against global-selection changes, absent/deleted receipts, stale client/owner fencing, headless Submit rejection and strict local DTO validation. Active desktop tests cover legacy flags/path rules, local selection, ACK/apply cancellation, stale ownership and malformed replies. Six obsolete planner-only tests and their mock resource provider were removed with the unused facade; no behavior tests were deleted to preserve counts. Pinned discovery/Submit/Receipt-loop regressions belong to the authoritative Service local-launch helper, not a duplicate desktop implementation.

The cleanup checkpoint removed `app_launch_forward.rs`, `app_launch_forward_tests.rs` and the unused coordinator `token`/`has_pending` accessors. The live `LaunchFuture` alias moved into the coordinator and remains re-exported from `app_launch` for `launch_runtime.rs`. No production caller changed.

Current cleanup validation:

- Production `cargo check --locked -p shellfox-desktop --bin shellfox-gpui-experiment`: passed, no launch-helper warnings.
- `cargo test --locked -p shellfox-desktop --bin shellfox-gpui-experiment app_launch:: -- --test-threads=1`: all 10 retained parser/coordinator tests passed.
- Three existing pure startup/dispatch/bootstrap-destination tests passed from the emitted unit/deps harness.
- Scoped warnings-denied Clippy of the actual launch-helper test root against current emitted metadata passed.
- Full production Clippy exited 101 on 27 outside-scope app/settings/field warnings and lints. No launch-helper lint or suppression was introduced; no whole-bin Clippy pass is claimed.
- Owned-file formatting/whitespace checks passed. Root, Options, startup and launch-runtime sources were not edited by the cleanup.

Historical pre-cleanup READY checkpoint:

- `cargo test --locked -p shellfox-service --lib desktop_launch:: -- --test-threads=1`: 17 tests.
- `cargo clippy --locked -p shellfox-service --lib -- -D warnings`: passed.
- Private `rustc --test` compilation of the actual new `src/app_launch_test_root.rs` against current emitted dependencies, filtered to `app_launch::`: 16 tests.
- Scoped `clippy-driver` for that actual helper test root with warnings denied: passed.
- Owned-file formatting and whitespace checks passed. Every new Rust file is below 350 lines.

Those historical 33 tests included six planner-only tests and are not the current cleanup count. The private root excluded unrelated lifecycle/UI modules without replacing launch source or schema. A historical desktop `--bin` attempt exited 101 on six `ShutdownOutcome.presentation` compatibility errors in stable AppHost tests/patterns and `root_close.rs`; the later cleanup production check and targeted tests passed after those owners' updates. No outside-scope fixes were made by this task.

Historical command-wide 101 results include another worker's runtime external-source wiring, the missing ResizeExternal match arm, private-client wiring, Options pattern guards and the automation stopping-fence signature transition. Tests were paused for Service READY rather than repeatedly running known intermediate errors. No fence argument was dropped, stub added or outside-scope error patched here. The helper tests also caught Serde silently accepting extra fields on an internally tagged unit variant; that was fixed with strict empty-struct variants, not waived.

The negative remote-trait compile-fail doctest passed at an earlier checkpoint. Its current refresh failed abnormally because the emitted `shellfox_terminal` rlib was missing before rustdoc could run; this is not a current doctest pass. Shared build-artifact invalidation also blocked one earlier private check. These are reported separately from source/test results. Shared namespace admission's 69 client passes and HOST receipt's 89 passes/Clippy are attributed worker checkpoints, not reruns here. No Core/mobile schema, public remote allowlist, root lockfile or dependencies were changed. Native supported-platform and full Explorer/CLI interaction acceptance remain open for the now-wired entry points and callback, using a separately approved owned fixture. No visible GUI automation or protected-process interaction was performed.
