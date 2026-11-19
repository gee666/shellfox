# GPUI accessibility integration

## Current status

The desktop uses AccessKit native providers and measured GPUI semantic elements. Root owns the provider and one stable `WindowSemanticContext`. Fields and the terminal collect into that same frame. Environment suggestions and Root dialogs now use the real deferred and overlay boundaries.

The latest focused Windows checks passed:

| Check | Result |
| --- | --- |
| Desktop `--bin` filter `accessibility::` | 77 passed |
| Desktop `--bin` filter `terminal_semantics::tests::` | 5 passed |
| Desktop `--bin` filter `settings::semantic_tests::` | 3 passed |
| Production desktop check | Passed, no accessibility warnings |
| Production desktop Clippy | Passed, no accessibility warnings |

The production checks still reported unrelated desktop and dependency warnings. They are not warning-free whole-application results. The previous 83-test result included obsolete cache and inspection APIs and remains historical evidence.

These checks did not activate a screen reader, exercise live UIA/AT-SPI/NSAccessibility actions or establish pixel parity. Windows, macOS and Linux native acceptance remain open. The earlier macOS check stopped in `ring` because an Apple C compiler was unavailable on the Windows host.

Visible automation remains paused until an exclusive owned test window is agreed. Processes 9484, 25904 and 38832 are user-owned and protected. Do not inspect, input, capture or close their windows. Earlier owned-window paint investigations are recorded in `docs/ghostty-rust-worklog.md` and `tmp/desktop-ui-acceptance/`; they do not establish cleanup or acceptance for another process.

## Public APIs and dependencies

The integration uses GPUI 0.2.2 and registry AccessKit code, not Zed HEAD.

- GPUI has no public accessibility tree or action-provider API. Its `Window` implements `raw_window_handle::HasWindowHandle`.
- `SemanticElement` delegates the child's element ID, source location, layout, prepaint and paint. It adds no layout node, styling, drawing, hitbox or input handler.
- AccessKit Windows `SubclassingAdapter` handles `WM_GETOBJECT`, focus and destruction through its maintained window procedure.
- AccessKit macOS subclasses the actual NSView, retains it and restores its class on drop.
- AccessKit Unix publishes AT-SPI over D-Bus. Wayland global window position is unavailable and must not be invented.

The desktop-only dependencies are pinned:

| Package | Version | Purpose |
| --- | --- | --- |
| accesskit | 0.25.2 | Tree and action contracts |
| raw-window-handle | 0.6.2 | Borrow the live native window |
| accesskit_windows | 0.35.2 | Windows UI Automation |
| windows | 0.62.2 | HWND and visibility checks |
| accesskit_macos | 0.27.2 | NSAccessibility |
| accesskit_unix | 0.24.1 | Linux AT-SPI |

They remain outside Core, Host, CLI and mobile. Do not add `accesskit_winit`; GPUI does not use winit. AccessKit's Rust 1.87 minimum fits this workspace's Rust 1.97.1 pin. GPUI's Windows 0.61 dependencies coexist with the direct Windows 0.62 family.

AccessKit declares MIT OR Apache-2.0. Full upstream license texts were verified at commit `f3026222559c7f5f46d8f25cb7404de68eb149f0`:

- LICENSE-MIT SHA-256 `23f18e03dc49df91622fe2a76176497404e46ced8a715d9d2b67a7446571cca3`
- LICENSE-APACHE SHA-256 `62c7a1e35f56406896d7aa7ca52d0cc0d272ac022b5d2796e7d6905db8a3636a`

Packaging must retain them and complete the transitive dependency audit. Source headers and license declarations alone are not packaging clearance.

## Window ownership

Create the native provider before showing or focusing the window. `HiddenWindowPresentation::prepare` returns hidden, unfocused options and a presentation plan. Disable macOS automatic tab grouping with `tabbing_identifier: None`.

Root retains the provider for that window's lifetime. OS callbacks hold only a shared validated snapshot and a bounded action queue, never a GPUI Window borrow or another Root.

Normal presentation uses public activation. Windows acceptance uses the queued no-activation path and retains its Task. The callback releases App/Window/Root borrows before native placement, then checks the actual GPUI viewport against native client bounds. It never assigns a guessed viewport, steals focus or repairs another window.

The provider checks its window cookie, original GPUI window ID, native/Rust thread and own-process identity. It exposes no arbitrary HWND/PID parameter. Teardown removes only its own matching cookie. Native destruction tracking makes teardown safe in either Root/window order.

Nonactivating acceptance on macOS and Linux remains explicitly unsupported. That is separate from their normal production provider implementation and still requires native testing.

## Stable collection and IDs

Construct one context in Root, not a new collector in each render:

```rust
let semantic_context = WindowSemanticContext::new(root_id);
let frame = semantic_context.frame();
let close_id = frame.id_for("settings/close")?;
let rows = frame.reserve_ids("terminal/session/generation/rows", 250)?;
```

The constructor protects the root ID. `id_for` and `reserve_ids` use exact logical keys, not hashes. Cloned frames share the same window-lifetime registry. A key/group conflict, changed group length or exhaustion is an error, never permission to invent an ID. Keys must include the logical terminal/document generation, not coordinates, labels or secret values.

Explicit `reserve_id` and the unused context forwarding methods were removed. There is no production cache replay API.

Wrap controls with `frame.wrap(node, original_element)`. Use their real `FocusHandle` and actual paint bounds. The collector owns parent/child links, so do not insert the same children again into NodeData.

Use one outer `frame.finish(original_root, commit)` for the whole window. It begins collection before normal paint and completes through public `Window::defer` after the draw and borrow stack return. Do not publish after normal paint alone, reset collection during render/prepaint or add a second finish inside Settings.

Superseded and cross-context callbacks cannot publish. An invalid frame cannot replace the last complete native snapshot. Root reports the error and requests a valid fresh frame. Repeated failure must not leave a stale tree that claims to describe the current session.

The first measured insertion also binds the collector to the actual GPUI window. `tree()` and fixture snapshots are test inspection helpers, not production publication methods.

## Uncached Settings and overlays

Settings currently returns an ordinary GPUI entity view. It does not use `AnyView::cached`. The former retained semantic cache wrapper and its field/init were unnecessary and have been removed.

Do not add `.cached` to a semantic view until a real measured semantic-replay design is implemented and tested. The current collector depends on actual descendant paint callbacks. It cannot replay an old tree at a guessed location.

GPUI paints normal roots before deferred children and built-in overlays. A high-priority deferred sentinel is not a final publication point. Use the root's end-of-draw completion and the two current wrappers:

```rust
// The child already contains its semantic root.
frame.deferred_subtree(overlay_root_id, original_child, original_priority)
frame.overlay_subtree(existing_semantic_dialog)
```

`deferred_subtree` registers the expected root during normal paint. The actual overlay paint must measure that same ID or the frame fails with `UnpaintedOverlay`. A placeholder in normal content does not satisfy it.

The Environment anchored popup keeps its original semantic ID and priority 50. Existing Root backdrop/dialog content uses `overlay_subtree`. These wrappers do not change layout, position or pointer behavior, and do not add duplicate semantic nodes. Late callbacks, missing painted markers and scope unwinding have focused tests.

Native OS dialogs have their own providers. They still need platform acceptance.

## Native action routing

Drain provider actions on the existing UI timer, including idle periods. Activation may request a new frame. Resolve IDs against current state and call the existing focus, editing, disclosure and selection handlers. Do not synthesize coordinate clicks, shell commands or a second privileged path.

The bridge rejects hidden/disabled ancestors, missing nodes, oversized text, nonfinite values, invalid selections and stale text snapshots. It permits only advertised actions with typed payloads. Its queue is bounded to 64 requests and 16 MiB of text/snapshot data. A replacement may reach Electron's 8 MiB editor limit; the editor must also validate its own UTF-8 and storage limits.

Native updates contain changed nodes and current focus. Full activation returns the latest complete tree. Removing children updates their parents.

## Terminal stamps and selection

A terminal ViewStamp is private typed metadata on the validated tree. It is not an AccessKit name, value or class name. `TerminalSnapshot.terminal_stamp` belongs to the same sampled text, runs, caret and selection.

During paint:

```rust
let snapshot = terminal_document.snapshot_view(
    &presentation, geometry, read_only, now,
)?;
let stamp = snapshot.terminal_stamp.ok_or(SemanticError::TerminalStamp(terminal_id))?;
frame.set_terminal_view(terminal_id, stamp)?;
```

Insert the document and measured TextRun nodes from that returned snapshot. Do not attach a newer getter's stamp to coalesced old text. The old `snapshot(&Screen, ...)` is fixture-only.

Publication commits the stamp even when NodeData and native focus are unchanged. Native callback admission captures it under the same snapshot lock as action validation. A queued action never adopts a newer stamp when drained.

Root calls `selection_action_for_view(range, action.terminal_stamp, current_frame)`. Missing or stale stamps request fresh publication and reject the original intent. Successful conversion preserves the sampled version in `PresentationAction::SelectionAtVersion`. Runtime also checks its source revision. There is no retry, Claim, detach, PTY write or cursor movement to rescue old coordinates.

Selection follows `crates/ghostty-vt/PRESENTATION_SELECTION.md`:

- Endpoints are inclusive physical cells, preserving anchor/focus direction.
- One actual `Cell.text` is one selectable-unit entry. Do not re-cluster adjacent cells or split a cell's combining/ZWJ text.
- Mode 2027 can produce different cell entries for the same concatenated text.
- Wide tails resolve through their known leading span. Unmapped spacers have no guessed boundary.
- Offscreen or rectangle endpoints produce no fabricated visible selection. None remains unset-or-pruned as reported by the producer.
- Collapsed AT boundaries clear view selection, not the PTY cursor. Unrepresentable LF-only or reversed coincident ranges return unavailable.

The removed `view_stamp`, `caret_history`, `presentation_row`, `selection_known` and old coordinate-conversion helpers are not live APIs. Tests and consumers use the coherent returned snapshot and production guarded mapping instead.

## Text and geometry

Terminal paint uses actual canvas bounds, font advances, line height and the published ViewFrame. AccessKit exposes a Terminal document with a read-only text model and TextRun children. It does not expose ReplaceSelectedText or SetValue for the terminal, even when interactive. Keyboard input still needs the normal control lease.

Soft wraps join without synthetic LF. Hard row breaks add LF. Physical row bounds remain separate. Wide continuations do not duplicate text; invisible cells expose spaces. A cell cluster longer than AccessKit's one-byte selectable-unit length is rejected, not truncated.

Within one identity/private view/grid, text, caret and selection coalesce to 50 ms samples. Geometry updates each frame. Binding or grid changes invalidate the sample. The UI timer requests the final coalesced paint when due, without a permanent redraw loop.

The current limits are 250 visible rows, 500 columns and a 16 MiB screen-text budget. The complete semantic tree permits 32 MiB for terminal/editor/control text. Native latency and sustained-output memory still need measurement.

Small fields and editor documents use measured shaped-line TextRuns with their actual editing offsets. Password fields expose masked text, never the stored secret. A name/value wrapper alone does not prove text-range support.

The desktop asset editor now uses one app-scoped journaled `EditorBackend` under the validated stable `desktop_dir/file-editor` namespace. Open and revision-checked save transfer 192 KiB chunks, up to 8 MiB. Immutable snapshots, compact byte/UTF-16/line indexes and bounded visible shaping avoid copying or scanning the full document on each paint. Single-line fields keep their 8192 limit; generic textareas and inline APIs keep their 128 KiB limits. The focused editor checks passed 5 backend, 18 Field and 4 file-state tests. These checks do not establish native or pixel acceptance.

Large-document accessibility is still unavailable. Asset documents over 128 KiB or 4096 rows expose an unavailable Group, without TextPattern, value, replace or selection actions. Visible snippets must not masquerade as the full document. Full-document native ranges, values, selection and scrolling need the separate document-backed provider work and acceptance on Windows, macOS and Linux. Desktop-wide Clippy was blocked by the concurrent profile-catalog migration at this editor checkpoint.

## Behavior and acceptance

Preserve the source controls' names, roles, disabled/selected/expanded states and keyboard handlers. Roles do not implement navigation by themselves.

- Session list items remain pressed buttons. Virtualized navigation reveals destinations without swallowing editing keys.
- Terminal tabs retain roving focus, rename, reorder and context-menu behavior. Close buttons use the actual terminal title.
- Settings categories retain their source names and roving stops. Hidden panels are inert and absent from the tree.
- Folder suggestions keep input focus in the combobox and expose the measured listbox/active descendant.
- Files expose the tree, toolbar, editor, splitter and dirty confirmation. Save and text selection use existing validated handlers.
- Modal dialogs contain focus, close on the source's keys and restore surviving prior focus. Menus retain their source navigation and dismissal behavior.
- Decorations and brand artwork stay hidden from AT. Missing/unknown status is not a fabricated count or live process claim.

Use safe hashed unit artifacts:

```text
cargo test --locked -p shellfox-desktop --bin shellfox accessibility::
cargo test --locked -p shellfox-desktop --bin shellfox terminal_semantics::tests::
cargo test --locked -p shellfox-desktop --bin shellfox settings::semantic_tests::
```

Do not run the desktop `--test accessibility` target while a regular desktop executable may be loaded. Cargo may build and replace that normal binary as an additional target. For actual application checks, use the private BuildPlan output, not `cargo run` or a shared loaded executable.

Live acceptance requires a newly authorized owned window and process birth identity. Test provider reachability, roles/names/states, measured bounds at each DPI, focus/edit/selection round trips, stale-action rejection, lifecycle teardown and bounded events under sustained output. Windows needs UIA; macOS needs NSAccessibility/VoiceOver; Linux needs AT-SPI/Orca on both supported window systems. The AccessKit macOS ListBox selection issue noted upstream as #520 still needs native verification.

Strict Electron/GPUI pixel comparisons remain separate. Passing these unit checks or preserving layout delegation does not establish either visual or OS accessibility parity.
