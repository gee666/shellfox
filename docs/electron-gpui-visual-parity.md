# Exact Electron visual and functional parity

## User requirement

> interface should be pixel-perfect exactly the same as electron app now. And functionality.

This is a literal acceptance requirement for the GPUI replacement. Match the current Electron desktop interface in every UI state and preserve its full functionality. Approximate styling, a simplified layout, reduced UX, missing controls or a redesigned workflow do not satisfy it. Any deviation requires explicit user approval before acceptance; technical difficulty is not approval.

The complete migration remains in progress. Existing GPUI unit/Clippy results, earlier 18/25-test reports, owned native-window checks, terminal screenshots and successful file saves are useful implementation evidence. None is pixel-perfect acceptance without a matched Electron reference and the checks below.

## Reference application and change control

- Use the committed Electron desktop implementation in this experiment worktree, not the new GPUI UI or the original dirty checkout as the reference. The inspected baseline is `c8cafb86d3a3b1bae01c3e12ed13df4fb4636dfd`.
- Reference actual `src/renderer` components/styles/assets, preload/main behavior, menus, dialogs and OS window integration. A screenshot of a mockup is not an Electron golden.
- Record the exact Electron commit, source tree status and build mode with every golden set. If the approved Electron baseline changes, review and version the fixture/reference update explicitly rather than silently changing expected pixels to match GPUI.
- Do not import or overwrite uncommitted mobile work from `../shellfox`. That checkout remains separate and requires coordination. Desktop parity does not authorize an unreviewed mobile UI rewrite or dropping mobile functionality.
- Parent coordinates the isolated Electron visual-baseline worker; Eva owns exact GPUI UI rework against those references. Coordinate shared runtime/input/renderer edits with their owners. Do not modify AGENTS files.

## Controlled golden fixtures

Capture the actual Electron application with a new, explicit isolated user-data/host/database/project directory under this worktree's `tmp/`. Never connect the fixture to installed Shellfox, an existing host, user credentials, a real approved peer or production session data.

For each reference, record:

| Control | Required recorded value |
| --- | --- |
| Application | Commit/tree state, Electron/Node version, build/package mode, fixture version and launch command |
| Platform | OS/version, architecture, display server/compositor where applicable, GPU/driver and display identity |
| Pixels | Monitor scale/DPI, logical and physical window/content bounds, screenshot dimensions, zoom and any system text scaling |
| Window | Position, restored/maximized/fullscreen state, active/inactive focus, native/custom titlebar mode and system theme |
| Typography | Exact font files/family/fallback/weight/size, text metrics/line height, locale, input layout and text direction |
| Appearance | App theme, accent/background, OS dark/light/high-contrast/reduced-motion settings when supported |
| State | Selected machine/session/tab/file, sidebar/file panel sizes, settings/dialog/menu state, scroll positions and focus/selection |
| Data/time | Stable IDs/timestamps/titles/paths/content/status/process facts, fixed clock and controlled output/transfer/connection progress |

Run the GPUI candidate on the same OS/display configuration and fixture state with the same DPI, fonts, theme, window dimensions/state and timing. Do not compare one platform's GPUI screenshot to another platform's Electron screenshot.

Use deterministic test-owned services, project files, fake process/remote facts and terminal fixtures to reach states safely. These are state-control fixtures, not replacement screenshots or permanent disconnected application code. Interactions under test must still execute the real implementation.

Control cursor blink, animations, timers, spinner phases, timestamps, PIDs, network status and terminal output through fixtures or a fixed observation point. Do not mask terminal contents, status indicators, labels, icons, notifications, window controls or any other required content to make a diff pass. Do not hide a difficult widget or capture only a convenient crop. Unavoidable nondeterminism must be documented and explicitly approved by the user; no silent tolerance or masking policy.

Preserve full-window references, the matching candidate, image diff/overlay and a machine-readable environment/state manifest. Keep disposable captures in `tmp/`; version accepted fixture definitions and deliberate golden records through the baseline worker's agreed path. This document does not claim captures already exist.

## Required visual-state inventory

Enumerate every reachable Electron UI state, not only the examples below. Map each state to its source component, deterministic setup, capture ID, matching GPUI state and result. All entries start open.

| Area | States/content that must match |
| --- | --- |
| Window/chrome | Titlebar, icon/title, frame/borders/shadows, native/custom menus, minimize/maximize/restore/close controls, active/inactive, restored/maximized/fullscreen, window resize and multiple windows |
| Navigation | Local/remote machine list, expanded/collapsed sessions sidebar, resizing/minimums, selection, hover/pressed/disabled/focus states, scrollbar and keyboard navigation |
| Sessions | Empty/loading/error states, create directory prompt/completion/validation, titles/path truncation/tooltips, running/waiting/unknown/error/settled statuses, pins/history/pagination, rename/delete/settle confirmations and context menus |
| Tabs/workspace | Empty/loading/launching/closed states, active/inactive/hover/focus tabs, rename/reorder/close, profile/elevation menu, badges/agent indicators, files/folder controls, takeover/blocked/reconnecting and read-only presentation |
| Terminal | Font metrics, grid placement/padding, colors/background/cursor/attributes, wide characters/emoji, selection/scrollbar, links, scrollback, mouse mode, focus/preedit, error notices and resize behavior |
| File browser | Open/closed/resized panels, tree hierarchy/icons/indentation, path entry/breadcrumbs, selection/hover/focus, collapsed/expanded nodes, empty/loading/error, create/rename/delete dialogs and context menus |
| Previews/editor | Text/code gutter/line numbers/caret/selection/wrap/scroll, toolbar, source/preview toggle, Markdown/HTML/SVG/image/binary/error/loading states, image editing, dirty/conflict/reload/save/discard states and file-line navigation |
| Transfers | Upload/download controls, pending/progress/completed/cancelled/error states, filename/path/size and permission feedback |
| General settings | Layout, section/tab selection/focus, all colors/swatches/font controls present in the approved baseline, history/retention/updates, saving/saved/dirty/validation/error states |
| Terminal/agent settings | Default profile/unavailable choices, environment modal/table/editor, host OS validation feedback, Python setting where present, built-in/custom rules and agent activity |
| Remote access/SSH | Network/login/connecting/online/error, peer discovery/remembered/approval/rejected/revoked states, permissions, SSH profile list/edit/import/key/known-host confirmation and unavailable/error feedback |
| Dialogs/menus | Directory/file pickers, close/quit/update confirmations, inline/modal/context/application menus, all button states, focus order, dismissal and default action |
| Global feedback | Notifications/toasts/status, busy/error banners, keep-awake, updates, disconnected state, tooltips and keyboard shortcut hints |

Do not add fields or visuals absent from the approved reference merely because the new Rust model has them. Conversely, a missing host implementation is not permission to omit its Electron control. Record the dependency gap and implement it.

## Comparison and acceptance

1. Capture the Electron golden and GPUI candidate in the identical controlled state.
2. Compare the whole-window images pixel by pixel and retain diff/overlay artifacts. Pixel-perfect is the requirement; do not quietly choose a permissive similarity threshold. Any proposed tolerance/deviation needs explicit user approval with evidence of its cause and affected states.
3. Independently compare geometry: physical/logical bounds, padding/margins/gaps, alignment, radii/borders/separators, menu/dialog placement, icon sizes, scroll positions and window chrome/control geometry.
4. Verify typography/assets: font files/fallback, weight/size/metrics/baselines, wrapping/ellipsis, glyphs, colors, opacity, icons and terminal attributes. Reuse existing validated assets where appropriate rather than replacing them with approximations.
5. Check focus/hover/pressed/disabled and accessibility states through interactions, not only static selected-state screenshots.
6. Link the visual result to the separate interaction/behavior row below. A perfect static screenshot cannot prove functionality; a working button cannot prove pixel identity.
7. Record OS-specific native results, unresolved differences and user-approved exceptions explicitly. A missing golden or blocked environment is unverified, never a pass.

Native rasterization/font/windowing differences must be solved or raised for an explicit user decision. Do not call them automatically acceptable because the frameworks differ. Never update a golden to the GPUI output merely to make a comparison green.

## Independent interaction and functionality matrix

Maintain a separate behavior matrix, linked to the architecture parity checklist. At minimum each row records the Electron behavior, GPUI path, fixture/setup, actions, observable result, state/event/error/persistence effects, keyboard/mouse/accessibility behavior, OS evidence and blockers.

Required coverage includes:

- Create/select/rename/pin/settle/unsettle/delete/reopen sessions; tab create/profile choice/rename/order/close/retention; confirmations, failures and idempotent retries.
- Local/remote machine switching, approvals/revocation/takeover/read-only behavior, hidden/blocked views, disconnect/reconnect, process/agent/status/stats updates and no loss of terminal ownership.
- Terminal input, IME, shortcuts versus shell Ctrl+C, mouse/selection/copy/paste/links/scroll, resize/output ordering, full TUI and recovery behavior. Preserve the separate public-ABI full-state recovery blocker; visual parity cannot waive it.
- Files navigation/preview/edit/save/conflict/drafts/image editing/transfers/cancel and path/permission/revocation checks, including remote and WSL scopes.
- Every settings category, validation/save/error behavior and persistence, environment precedence, notifications/updates/keep-awake, native dialogs/window menus/controls, multiple-window focus and confirmed shutdown.
- Accessibility names/roles/states/values, focus order/restoration, screen-reader access, keyboard operation and supported system settings. Accessibility equivalence requires semantic tests, not screenshot inference.

No simplified workflow, inert control, alternative shortcut or reduced error/confirmation behavior is accepted without user approval. Full functionality includes privileged host/CLI/mobile contracts and the full migration backlog, not only the visible screenshot.

## Platform evidence and current status

- Windows, macOS and Linux GPUI desktop must each match that OS's actual Electron reference with native capture and interaction evidence. Keep x64/arm64 build/native support obligations explicit. Cross-compilation, CI configuration or a Windows screenshot does not establish another OS's visual parity.
- Headless behavior remains independently tested with no GPUI/display dependency. Mobile retains its approved RN interface and full client functionality while both native terminal implementations are validated. Do not infer Android/iOS visuals or accessibility from desktop proof or import dirty mobile changes without coordination.
- No platform can be silently removed, redesigned or assigned a reduced acceptance bar. Ask the user before any departure from the literal requirement.
- At this requirements update, controlled matched Electron/GPUI golden acceptance has not been supplied. Parent is launching the visual-baseline work and Eva's exact UI rework is ongoing. Prior GPUI 18/25 unit reports and native-window/file-save proof remain progress only.

Pixel-perfect visual comparisons, independent functional interactions, accessibility and native platform evidence must all pass, or have a specific explicit user-approved deviation, before desktop replacement acceptance. This requirement does not authorize production-data use, releases, full-suite reruns or AGENTS edits.
