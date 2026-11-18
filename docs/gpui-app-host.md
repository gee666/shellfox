# GPUI app host and window helpers

These new modules belong to the desktop adapter. They do not add GPUI to the service, open a second Host, or change the current window style. Eva owns Root/main integration. Tughril owns the endpoint and typed startup/cleanup contract.

## Source policy

Read against this worktree's `c8cafb8` Electron baseline and current Rust implementation:

- `src/main/index.ts`, `attached-desktop.ts`, `terminal/shutdown.ts` and `close-confirmation.ts` define app quit confirmation and confirmed disposal. Ordinary quit with zero owned terminals needs no terminal prompt. Cancellation does not dispose the service. Failed terminal disposal retains the host for retry.
- `src/renderer/store.ts` keeps selected session and active tabs in the client. It repairs removed entries, ignores older snapshots and refuses to redirect a missing explicitly restored tab into another shell. `TerminalTabs.tsx` chooses the right neighbour, then the left, when closing an active tab.
- `TerminalViewport.tsx` and `terminal-control.test.tsx` define the blocked terminal and explicit "Take it" action. A denied controller is not a read-only observer.
- `src/main/window-state.ts` defines the five persisted logical-pixel fields, 1220x820 defaults, 850x600 minima, title-strip visibility and 400 ms saves. Fullscreen/maximized capture uses normal bounds. Minimized/destroyed windows do not overwrite them.
- `src/main/session-windows.ts` concerns authenticated external Windows Terminal generations. These helpers do not revive that retired backend or translate an uncertain close into a launch.
- `HostClient::fork`, `NativeClient::fork`, `OwnerClient::fork` and `Service::native_detach_all` are the actual shared-owner client path. The owner allocates controllers from fresh UUIDs. UI code never allocates a controller number.

`Snapshot.selection` is persisted recent selection in the Rust core. It is not a command to change every open window's active tab.

## Application ownership

Install once before creating normal workspace Roots:

```rust
let host = app_host::adapter::install_with(cx, config, |config| {
    app_host::AppHost::start_with_dispatcher(config, dispatcher)
})?;
```

The supplied dispatcher is the existing service/native adapter, including its automation and remote integration. Repeated installation with the same configuration returns the Global's host without invoking the factory. A different configuration is rejected. Resolve one explicit experiment data directory before installation; do not fall back to production data or an alternate spelling after a startup error.

Each native window awaits `host.open_window()`. Before native construction, check `host.is_window_available(id)` again on the UI thread so a late startup callback cannot create a window after app quit closed its registration. Keep the returned `WindowClient.id` and `WindowClient.client` in that Root. Every allocation, including the first, calls the base `HostClient::fork()`. Cloning that returned client shares its window identity and is appropriate for terminal views and settings within that window. Never call `HostEndpoint::start` from a Root or fork a client for every repaint/tab.

The Global retains the endpoint independently of views. CLI/private IPC and remote adapters still use the same service owner. Closing a window sends only that UUID's `DetachAll`. It does not call runtime `RetryClose`, service Stop, helper power release or a PID-based cleanup operation while another window remains.

The registry holds at most 64 window clients. Successful close removes their registry entries. Frame caches and request budgets stay in the existing per-window `HostClient`. Do not create an app-wide map of frame history. The local active-tab cache retains at most 128 session entries and prunes deleted/archived entries.

## Close ordering

One app coordinator handles native close, menu close, keyboard close and app quit through the same API:

1. Update each registered window's `WindowCloseGuard` whenever its hidden or visible file draft, file operation or metadata operation changes. Do not derive dirty state from whether the file sidebar is open.
2. Await `host.owned_terminal_count()`, then call `request_close(CloseTarget::Window(id), owned_count)` or `request_close(CloseTarget::Quit, owned_count)`. The backend queries actual `NativeAction::Status` and reads `NativeResponse::Status(NativeStatus { active_terminals })`, populated by `Service::active_terminals()`. It never counts persisted tabs or GUI views. Retained/unknown startup is an error, not zero. Only source-proven NoOwnerClean supplies zero without RPC. `Blocked` leaves the window and host untouched. Reveal the relevant existing file/error UI.
3. `Confirm` uses the existing quit modal. Only its affirmative response calls `CloseConfirmation::approve()`. Dropping the confirmation is cancellation. A secondary clean window receives `Ready` without a host-quit prompt.
4. Call `begin_close(&ticket).await`. This reserves targets without detaching or stopping anything. If another window closed or a draft changed during the prompt, the old ticket fails before any host cleanup. Recheck policy instead of silently upgrading a secondary close into host shutdown.
5. Freeze input, edits, terminal creation and host mutations in the returned target Roots. Drain their native terminal outboxes, including reserved key/mouse releases, through the existing `TerminalView` retirement/close path. Await the drain before final `DetachAll`. Service detach alone cannot drain a UI outbox. If this stage fails, `abort_close(&ticket)` releases the reservation for retry and does not Stop the host.
6. Await `finish_close(ticket)`. It releases each target's client, then awaits endpoint Stop and actual owner/helper completion only for the last window or app quit. A cancelled wait preserves its reservation and completed detaches; repeat the same ticket to resume. The endpoint preserves its pending Stop receiver, so resuming does not enqueue another Stop.
7. Only `ClosedWindows` authorizes native window removal. Only `app_stopped == true` authorizes `cx.quit()`. Dispose each window's own accessibility provider/context after its window is authorized to close. Do not share one provider or collector between windows. At the current source checkpoint, `accessibility/frame.rs` exposes `WindowSemanticContext::id_for`, `reserve_id`, `reserve_ids` and `frame()`. Its frame is a shared window-lifetime collector with `begin_frame`/`finish_frame`, not a new collector to construct on each render. Caroline owns that implementation and Eva owns its Root wiring; source presence alone is not native acceptance.

`AppHost::is_stopped(&self) -> anyhow::Result<bool>` is the synchronous lifecycle getter for Main's `on_window_closed`. It reads only the app registry's stopped flag, set after successful final `finish_close` and backend shutdown. An empty OS window list, zero registrations, failed ready, retained cleanup, or a cancelled Stop wait does not set it. A getter error must not authorize quit. Eva's callback can require both no OS windows and `matches!(host.is_stopped(), Ok(true))`; it must keep the Global/coordinator for `Ok(false)` or `Err`. This helper task does not edit that callback.

`AppHost::shutdown_outcome(&self) -> anyhow::Result<Option<shellfox_service::ShutdownOutcome>>` retains the exact persistence report from successful final shutdown. `EndpointBackend::stop` awaits `HostEndpoint::shutdown()` and reads `shutdown_outcome()` while still holding the same endpoint guard, returning `Option<ShutdownOutcome>` through the injectable backend. AppHost stores the report before setting its stopped flag and logs `DirtyAfterOwnershipLoss { pending_sessions }` with that original count. The warning means session activity updates were not committed; it does not mean resources remain live. A missing report stays `None`, not a fabricated `Committed` result. Secondary detach, failed resource cleanup and a cancelled wait do not publish a successful app shutdown report. This activity warning does not discard editor drafts or bypass their separate close guards.

The endpoint owner reports all four shutdown-report bridge tests passing. Its report is stored before the clean-resource proof is published, so a clean failed startup can preserve a dirty-activity warning without RPC Stop. This is attributed endpoint evidence, not permission to infer cleanup from a thread/error or operate on older executables.

A returned cleanup error retains the endpoint and registered windows. It invalidates the old approval, permits a new policy/confirmation check and refuses new clients after failed app cleanup. Do not clear the Global, call unconditional `cx.quit()` from `on_window_closed`, kill a process by PID, or substitute an independent keep-awake/power state. Endpoint completion must include the real helper power-release proof.

A failed native window construction still leaves its registered client owned by the app coordinator. Release it through the same coordinator rather than losing it in a weak Root callback. If there are no visible windows, keep the application coordinator alive until confirmed cleanup succeeds.

### Startup failure contract

`AppHost::startup_outcome().await` forwards the actual `HostEndpointStartupOutcome` from `HostEndpoint::startup_outcome()`. It is a source proof, not a readiness/error flag. Walter's `local::LocalBootStatus` has `NoOwnerClean`, `RetainedCleanup` and `Ready`; Tughril's endpoint maps that typed source. The service must publish retained status before acquiring any helper/ownership handle, and Ready only after owner activation ACK plus endpoint publication. NoOwnerClean means pure validation rejected before handles, or explicit actor Stop/join, Go ownership shutdown and endpoint cleanup all completed. A thread error, thread join, missing client or empty registry is not that proof.

On source-proven NoOwnerClean, the close helper skips commands to dead window clients. Final app close still awaits `EndpointBackend::stop`, which calls the endpoint's `shutdown`. The endpoint handles this case without RPC Stop. RetainedCleanup keeps the same backend and registrations for retry. A failed `ready()` does not trap a zero-client window or authorize dropping the Global. Root should enter `CloseTarget::Quit` when startup never returned a window registration. `cleanup_retained()` describes a previous close failure; false is not a clean-startup proof.

The integration lead reports the typed boot guard checkpoint green: compiled service checks and three actual rollback tests passed, including corrupt-database failure after Go ownership acquisition and failure after offline `RemoteHostSetup` preparation. The new guard confirms actor join, borrowed remote shutdown, endpoint cleanup and original Go-child exit before publishing NoOwnerClean. Endpoint startup-outcome and clean-shutdown consumers are complete. This is attributed evidence for the new mechanism, not a rerun by this helper task or proof about an older executable. In particular, it does not authorize force-closing the preserved PID 39696 binary.

Before draining UI outboxes, recheck the typed outcome. If the source already proves NoOwnerClean, do not retry releases through dead clients. Drop only those retired UI intents and finish the policy-authorized close. If cleanup is retained, keep the app coordinator alive. Source-proof tests cover a failed ready wait, misleading error text, later explicit clean proof and existing windows whose clients are already gone.

## Window-local selection and control

Each Root owns a separate `WindowSelection`:

- `InitialSelection::Recent` consumes the first accepted snapshot's persisted selection once.
- `InitialSelection::Session(id)` is the explicit "Open session in new window" target, regardless of the host's recent selection.
- `InitialSelection::Tab { session, tab }` preserves a source window's chosen tab when that is the requested action. A missing restored tab yields no active terminal, never a different shell.
- `InitialSelection::Empty` stays blank until that window explicitly selects something.

Call `accept_snapshot` for revisioned updates, and render/focus from `selected()` and `active_tab()`. Do not overwrite `snapshot.selection` to fake locality. Explicit local clicks may still persist recent selection through existing metadata commands, but no other Root follows those writes. Bind delayed close/attach work to `epoch()` so it cannot focus a newer local selection.

`window_manager::control::attach(..., AttachIntent::Open)` always requests `takeover: false`. A typed runtime `ControlRequired` yields the existing blocked-terminal overlay. Only `AttachIntent::TakeIt`, invoked by the user's button, requests takeover. The helper never retries, calls Observe, fits a terminal, sends input or acknowledges a frame after a denial. Other errors remain real errors, even when their text contains "ControlRequired".

## Native adapter and persistence

`window_manager::adapter` supplies unit actions, optional key bindings, a window menu and window-options conversion. Install actions once and route them through the app coordinator. Open-session actions use the source Root/context menu's local session, not global recent selection. Merge menu items with the application's existing menu policy; the helper does not install a replacement menu or change the Electron baseline's hidden-menu behavior. The `Workspace` bindings are opt-in, not an excuse to consume terminal Ctrl+C or replace existing tab shortcuts.

The options helper preserves supplied titlebar, icon-related policy and other styling. Keep the reference product-version title and native DWM/icon setup in their existing adapters. It sets normal/maximized restored bounds, the existing minimum size and `tabbing_identifier: None`. Call `configure_opened_window` too, so macOS does not inherit a native tab group that breaks AccessKit's per-window provider mapping.

`window_manager::state` retains the Electron JSON schema's five fields: integer `x`, `y`, `width`, `height`, boolean `maximized`. Values are logical pixels. `WindowStateKey` separates device and stable local window slot; it is not a controller UUID or a remote path. The existing device-preferences adapter implements `WindowStateStore`, validates JSON types, writes atomically and bounds its saved slot collection. Do not add a second settings database.

Validate current monitors before restore. Use their work areas, not total desktop bounds. A saved position is retained only when at least 100 pixels of its 40-pixel title strip intersect a valid display with at least one pixel of vertical overlap. Otherwise center on the current primary work area and clamp size using the source minima. Do not clamp negative coordinates away when a real secondary monitor occupies them.

Native physical capture must use `capture_physical` with the monitor's actual physical work-area origin, logical work-area origin and current finite scale factor. Converting dimensions alone is wrong on mixed-DPI negative-coordinate monitors. Restore logical bounds through GPUI once; do not scale them twice. Schedule saves after move/resize/maximize/unmaximize, capture at the 400 ms deadline and capture normal bounds immediately on close/dispose. Hidden/minimized frames must not replace usable bounds.

## Verification and remaining gates

After the user's explicit wiring authorization, `main.rs` and `host_client_test_root.rs` declare the two new public modules. No Root behavior changed. A private headless `rustc --test` executable compiled the actual `host_client_test_root.rs` against the current Cargo-emitted dependency artifacts. Current filtered runs passed 23 app-host and 12 window-manager tests, including typed startup, actual owner count, stopped lifecycle and shutdown persistence reports. Five lifecycle regressions cover zero-window retained startup, zero-window failed Stop, a cancelled zero-window Stop wait, orphan registrations/secondary close and a poisoned registry. These exercise injected host commands, approval cancellation, stale/foreign tickets, input-drain reservation, per-window release, retained failures, cancelled waits, bounded clients/selections, local navigation, typed control denial, geometry, DPI and native options without constructing GPUI or opening PTYs/windows. Scoped `clippy-driver` with `-W clippy::all -D warnings` passed for the current 35-test checkpoint. Its first run was blocked by `clippy::collapsible_if` in shared `host_client_endpoint.rs`'s ready callback; the endpoint owner fixed that callback before the passing rerun. No shared-source edit or lint suppression was made by this helper task. New Rust files are formatted and below 350 lines.

The initial Cargo `--test host-client --no-run` prerequisite attempt unexpectedly tried to remove the shared `target/debug/shellfox-gpui-experiment.exe` and failed with Access denied. No process was stopped, and no test/application window was launched. That integration-test command was not repeated. The user then authorized the explicit `--bin` unit target, which emits a separate `.deps` test harness rather than the normal application. The private direct-compiler test executable was written only under this worktree's `tmp/` and removed after verification. Do not repeat the normal-executable integration-test command while a user-owned application is running.

`cargo test --locked -p shellfox-desktop --bin shellfox-gpui-experiment app_host:: -- --test-threads=1` passed 13 focused tests. The emitted `target/debug/deps/shellfox_gpui_experiment-8691febfe5f5b3c4.exe` passed 12 more with the `window_manager::` filter. No application/PTY test was selected. A later authorized `--bin ... --no-run --message-format=json` dependency-artifact capture exited 101 because concurrent `settings_semantics.rs` changes called a not-yet-present `SemanticFrame::reject` at three sites. That command is not a pass. The actual headless root compiled against its freshly emitted dependencies and passed the then-current 25 focused tests plus scoped warnings-denied Clippy. The app host/window helpers had no reported compile errors; whole-bin warnings and semantic integration remain their owners' work.

After adding `is_stopped`, the next authorized `--bin` run exited 101 on concurrent native accessibility provider calls to missing `validate_hidden`, `show_without_activation` and `validate_window` methods. No helper compile error was reported, and no Main/provider code was changed by this task. The actual headless root compiled privately against those newly emitted dependencies, passed the current 30 focused tests and passed scoped warnings-denied Clippy. This remains separate from a whole-bin pass.

The report-consumer check first exited 101 during an in-progress host storage split, then advanced to another command-wide 101 for the shared `NativeFrame.presentation` test constructor. Both source owners completed those changes without edits by this helper task. The private actual headless root then passed all current 35 focused tests, including five report regressions: dirty activity with confirmed resource cleanup, failure then successful retry, clean failed startup without RPC, absent versus explicitly committed reports, and secondary close/cancelled Stop ordering. These checks do not supersede the recorded Cargo failures or claim a whole-bin pass.

The existing headless test root now includes app/window modules for maintained coverage. Use an approved private artifact directory, or compile that actual root directly against the emitted dependency artifacts as above. Do not repeat `--test` against the shared normal executable. No manifest was changed. The only existing-file edits were the explicitly authorized module declarations in `main.rs` and `host_client_test_root.rs`.

Not claimed: Root/main integration, end-to-end GPUI quit and power-release acceptance, native two-window interaction, pixel parity, macOS AccessKit/native tabbing acceptance or Linux/macOS/Windows DPI monitor acceptance. An actual multi-window run needs Eva's separately approved newly owned fixture and private build. PIDs 9484 and 25904 were not sent input, captured, closed or killed. No TypeScript/native/full-baseline reruns were performed under the task's scope restriction.
