# Windows native backend

This backend creates an initial Windows Terminal window and tracks registered, non-elevated PowerShell shells on Windows x64. Terminal content stays in Windows Terminal.

Adding manager-created tabs is currently disabled. Verifier evidence showed a sixth shell delivered to an unexpected third window despite a title-verified existing target. This capability restriction prevents that unsafe dispatch; it does not satisfy the multi-tab release gate. Existing registered multi-tab windows can still be tracked and focused.

## Build and tests

Run from the project root, with an outer timeout:

```text
pnpm build:native
pnpm test:native
pnpm exec vitest run src/main/platform/platform.test.ts
node native/routing-safety-gate.mjs --run-gui
```

The focused GUI command needs permission to create two initial windows. It uses isolated fixtures in `tmp/routing-safety`, checks append refusal without new tickets/windows, actual target closure and null-target refusal after restart, then closes only recorded fixture processes and tabs. UI Automation is test cleanup, not a product targeting fallback. If unexpected tabs or children appear, cleanup stops rather than closing them. The historical `native/gate-windows.mjs` two-window/three-tab gate remains separate and cannot pass under this capability restriction.

Native publish output is `tmp/native/win-x64`. Ship the entire directory outside ASAR and ship `resources/shell/bootstrap.ps1` separately. The self-contained runtime is pinned to .NET 8.0.31. Build intermediates and package restores use `tmp/build-cache`. Testing on a machine without .NET remains part of packaged acceptance.

The factory is `createNativeBackend()` in `src/main/platform/index.ts`. It imports the shared contracts and runtime reply schemas. Other platforms and architectures return an unavailable probe and UNSUPPORTED native actions, never a fake terminal.

## Protocol and ownership

The broker uses private UTF-8 JSON lines on stdin/stdout, protocol version 1 and a 1 MiB frame limit. Ordinary bridge requests time out after 10 seconds. An exited helper rejects pending requests and is not restarted automatically. Closing manager stdin stops the broker, not terminal shells.

Each launch creates a 15-second ticket with a random 256-bit token. Ticket ACLs allow only the current user and SYSTEM. Registration uses a current-user named pipe and a transient child helper. The broker verifies the pipe client, actual shell executable, parent relation, creation time, user and elevation. Tokens are not command-line arguments, events or log data. A consumed or expired ticket cannot register again.

Only known standard PowerShell locations are offered. User profiles run normally. Folder data stays in the ticket and the bootstrap uses Set-Location -LiteralPath. WT receives explicit --window new and an encoded bootstrap for each fresh session. Session UUIDs remain title markers. The private DTO's windowName is retained as a compatibility intent tag, not a verified WT routing name, and is never used to append. No folder is interpolated into a command string or WT semicolon batch. Reparse-point folders are conservatively rejected; use the accessible local target folder.

Windows Terminal must expose its selected tab title in the title bar. The manager does not change this setting. A binding requires a unique exact managed UUID title and a Windows Terminal executable under its installed WindowsApps package path. Every control validates HWND, owner PID, exact FILETIME creation identity and session marker. Destruction events and polling invalidate targets. Foreground focus can return FOCUS_DENIED; there is no focus-permission bypass.

Named-window and numeric-ID CLI dispatch have no documented atomic existing-only operation. Title/HWND readiness cannot establish the CLI's internal lookup state. Appends are rejected with UNSUPPORTED before ticket creation or process dispatch. A flushed, create-new session reservation under userData/native-window-intents also prevents a caller from bypassing this guard with existingTarget:null after a crash, restart or uncertain launch. Reserved sessions must not be relaunched; check their terminal manually and create a fresh session. A reservation may conservatively block a retry even if the first spawn failed. It contains non-secret intent IDs only. No automatic retry, reassignment or claimed multi-tab fallback follows.

## Tracking limits

WMI snapshots run in the helper about every two seconds. Native FILETIME identities remain decimal strings, not JS timestamps. WMI creation dates are cross-checked before using PID, parent and command-line fields. Discovered lineage survives an intermediate launcher's exit during one broker run. Recycled parent PIDs cannot reassign previously observed children, and changing a registered shell identity clears its old descendant ownership even if the tab ID is reused. A newer registration or full watch replacement prevents an old batch from closing a retried tab.

Rules constrain executable identity and optional script-path component suffixes. Node alone is not a default agent rule. Node/Bun positional script slots and explicit PowerShell -File slots are recognized. Eval strings, imported-module option values and arbitrary user arguments are not searched for agent names. Unsupported required interpreter/option forms and inaccessible fields produce unknown monitoring rather than healthy waiting. One process identity counts once even if several rules match it.

Very short-lived processes may be missed. Orphans whose ancestry was already gone before the first snapshot cannot be safely assigned, including after broker restart. Detached, elevated, WSL, remote and multiplexer activity is not guaranteed. Agent counts are matching process identities, not logical tasks. Ordinary unmatched commands do not change the agent badge.

Specific tab activation, splits, attachment, closing terminals, stopping agents and command exit codes are unsupported. User tab/window renaming, moving tabs and selecting unmanaged tabs can invalidate control. Deliberately copying a UUID title is not a security boundary against the same user.

## Explorer integration

Installation is opt-in and requires a packaged executable. Managed registry APIs create only the folder and folder-background legacy verbs under HKCU Software Classes. Their commands use quoted `%1\.` and `%V\.` folder arguments to preserve drive-root argv parsing. Windows 11 may show these under Show more options. Files, virtual folders, remote folders and multi-selection are not supported.

Install refuses foreign ownership markers. Remove deletes only app-owned keys and preserves foreign ones. Main's Squirrel lifecycle must call the same operation on update/uninstall before taking the instance lock. Unit tests use isolated non-Explorer registry keys. Those tests do not prove actual Explorer clicks or packaged uninstall cleanup.

## Session window extension

`src/main/platform/session-windows.ts` defines the new native-only `SessionWindowBackend` extension. It can explicitly reopen a confirmed closed window into the same session through `--window new`, without replaying commands. `BoundWindow` carries the launch-operation generation; loss events and replacement requests match it. A live or unobservable old HWND is not treated as closed. Exactly one successor is reserved for an old generation, including after uncertain launch/restart.

Manual/new/moved PowerShell membership requires explicit `prepareRegistration` and running `resources/shell/register-session.ps1` in the selected terminal. No global profile is installed. A token-authenticated real shell/child helper, exact process identity, WT_SESSION connection metadata and visible challenge in the intended HWND/generation confirm association. Wrong-window challenges fail. A confirmed move transfers the existing shell/tab identity rather than duplicating it. WT_SESSION identifies a connection/pane, not its containing window; it cannot by itself discover moves. Title-suppressed tabs may need the supplied marker set through WT Rename Tab.

`getWindowMembership` enumerates authenticated members with last-confirmed bindings, not arbitrary manually created tabs. Untagged shells and unobservable moves stay outside automatic discovery. Closed roots/windows emit generation-qualified extension events. `restoreSessionWindows` is a trusted main recovery operation before live registration, and requires a live restored marker to verify. Foundation must persist generation/member DTOs and connect click-to-reopen, adoption intents, ownership transfers and renderer instructions. These service/UI changes are not part of the native pass.

Focused regression: `node native/session-window-gate.mjs --run-gui` from project root with an outer timeout and owned-window permission. Reports live under `tmp/session-window-gate`. See `tmp/handoff-window-reopen-adoption.md` and `tmp/report-b-window-reopen-adoption.md` for exact contracts, evidence and limits.
