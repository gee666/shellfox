# Shellfox Rust desktop workspace

This executable now opens a Shellfox workspace, not a terminal fixture by default.
It remains an isolated migration build. It does not replace the installed app,
connect to its host, import its settings automatically or open `shellfox.db`.

## Run

On Windows x86_64 use the worktree Rust 1.97.1 toolchain, a Visual Studio C++
build environment and Node.js 22+. The pinned official Ghostty DLL and import
library are verified by the build. No global toolchain or PATH change is needed.

```powershell
node crates/ghostty-vt-sys/build-support/prepare.mjs verify
node scripts/gpui-experiment.mjs
node scripts/gpui-experiment.mjs shell
node scripts/gpui-experiment.mjs fixture
node scripts/gpui-experiment.mjs smoke
```

The launcher uses `cargo rustc --locked` with a unique private output under `tmp/gpui-builds`. It does not replace `target/debug/shellfox.exe`, which may be a user-occupied window. Its DLL copy is hash-checked and includes notices. `shell` is the default and opens the
application workspace. `fixture` is an explicit parser/rendering fixture with
no shell or host database. `smoke` runs a controlled cmd script, independently of
the interactive default shell. `--release` selects a release build; it does not
measure performance or authorize a release.

Direct invocation of an already privately built executable:

```powershell
& "ABSOLUTE_PRIVATE_EXE" --data-dir "$PWD\tmp\gpui-host-v1" --desktop-dir "$PWD\tmp\gpui-desktop-v1" --helper "ABSOLUTE_HELPER_PATH" --helper-sha256 TRUSTED_BUILD_SHA256
```

The launcher accepts `SHELLFOX_EXPERIMENT_DATA_DIR` to choose a dedicated host root. Do not reuse a directory containing old GUI runs, logs or project fixtures. In particular, do not move or delete an active user's earlier experiment data to satisfy the new host's directory checks.

An absolute experiment-only `--data-dir` is mandatory. Metadata persists in
`<data-dir>/shellfox-rust-v1.sqlite3`; a lock prevents two hosts from opening that
store concurrently. It contains only the service's own artifacts. UI scratch goes to `--desktop-dir`, defaulting to the sibling `<data-dir>.desktop`. Argument parsing and new-window options are planning-only. After the typed ownership/handoff decision, rendering startup creates `run-PID-TIMESTAMP` there through the anchored frontend capability. Secondary handoffs create no frontend scratch. Host-root creation belongs only to native boot security, not the GUI parser. Logs and project fixtures stay outside the host root. This separation does not weaken the host's artifact allowlist or permission checks.
Device-local custom swatches use `--device-config-root ABS`. For the embedded experiment only, the default is the sibling `<data-dir filename>.device` in the same parent as `--data-dir`. It is not inside the Host root or UI run scratch. Host, UI scratch and device roots must be physically distinct and nonancestor. One native RW capability plans all five Host/UI/device/run/editor paths using the shared Files no-follow/native-identity walker. Retained root and ancestor identities, including conservative native-name proofs for missing paths, establish the topology before the first frontend mkdir; Unproven rejects without creation. All plans and relationships are revalidated before and after each create. Sensitive UI/run/device/editor roots use `create_private_approved_root`; missing components use the shared native private creator, while existing objects are verify-only, never ACL-repaired or certified private at birth. Ordinary user-folder creation is unchanged. Desktop never creates the Host plan. The swatch provider retains the exact returned approved-root capability for its background lifetime. The Host artifact allowlist is unchanged. One provider Arc is constructed and validated before GPUI and shared by all windows. Settings reads and atomic revision-checked saves run off the UI thread, retaining the last three colors independently for accent/background. The device regression checks metadata-store reopen and unchanged Host entries around device persistence. These checks do not establish native UI/pixel acceptance. Future attached/production composition must supply an explicit local frontend device root, never infer one from remote Host paths, executable paths, cwd, credentials or a production home.

The central service gives each terminal generation a separate private directory under its terminal-home tree for home, config, history, cache, temporary files and Pi state. Its selected project cwd is separate. Choosing a project directory never makes it HOME or APPDATA.

The launcher uses the ownership helper built under this worktree's `tmp/tailscale/<platform>-<arch>` and hashes that explicit local build. An overridden `SHELLFOX_EXPERIMENT_HELPER` also requires `SHELLFOX_EXPERIMENT_HELPER_SHA256` from its trusted build. The service verifies the helper rather than trusting a PATH lookup. No existing Shellfox application state or credentials are copied. Interactive
shells use the explicit `InstalledTools` environment policy, with a curated
absolute executable PATH and validated PNPM_HOME executable directory. Smoke
uses the minimal policy. Installed tools can still access files with your
account's permissions; this is not a filesystem sandbox.

A fresh Windows store persists its discovered default shell once, preferring
PowerShell 7, Windows PowerShell, then Command Prompt. Saved choices never fall
back silently. Unavailable profiles have visible reasons in settings. WSL is
unavailable until guest-process ownership is implemented. Unix interactive
support remains subject to its own process-tree acceptance checks.

## Workspace behavior

The UI is being ported against actual Electron captures, not accepted as pixel-identical yet. The reference is `tests/visual-parity/baselines/windows-x64-dpi96/electron-reference-final`. Geometry and assets come from `src/renderer/styles.css`, `terminal.css` and `settings.css`:
a 220px session list, 32px toolbar, 36px session rows, 30px terminal tabs, compact
controls and the existing pink accent on `#111016`. Colors and ANSI16 contrast
adjustments come from the same palette algorithm as `src/renderer/theme.ts`.
The terminal font is discovered from installed monospace fonts and measured by
GPUI. There is no bundled font or invented dashboard.

- New session uses the existing centered folder-path modal, native directory picker and real directory completions. The shared host derives its title from the chosen cwd. Rename uses the session action, and shell choice uses persisted settings.
- Selecting sessions or tabs retains existing runtime owners and screen state.
  Opening a saved workspace does not silently start all its terminals; activate
  a session or press Open terminal to start its selected tab.
- Add tab, rename, pin, settle, close and delete use the shared host APIs. A blue
  dot means an active shell, red means a reported failure, and settled sessions
  use the existing brown indicator. Agent detection is not implemented here.
- Settings changes font, size, accent, background and the default shell. Existing
  terminals receive live parser colors and geometry updates; subsequent tabs
  use the persisted profile. Errors remain visible instead of being swallowed.
- Files opens a local account-authority capability at the selected session cwd.
  Directory listings and bounded UTF-8 reads run off the UI thread. Plain text
  uses a native editor and revision-checked saves through `shellfox-files`.
  Unsaved changes require explicit discard before opening another file. Window
  close checks both dirty text and in-flight file operations, including while
  terminal cleanup is pending. This pane does not grant a network file capability.

NativeDesktop editor/files activation is statically blocked with `EditorUnavailableReason::RecoveryAuthorityPending` for both PathBound and Native routes. Main retains the same local Host/UI and installs only the typed admission outcome, never an EditorBackend. The gate precedes journal directory creation, lease acquisition, V1 recovery/rewrite and `.next` cleanup. Files shows the explicit user-stage recovery authority/V2 reason, without a lazy, path-compatible or unjournaled fallback. Legacy compatibility constructors/tests remain separate and unchanged. A private metadata root is not historical user-stage authority; activation awaits accepted actual user-stage authority and V2 schema, independently of current private-directory checks. Full editor AT parity also remains open.

The file pane currently supports navigation, text editing and saving. File
creation, rename/delete controls, image previews, search, uploads/downloads and
remote scope selection have not been ported into this desktop UI. Unsupported
links/reparse paths and non-text files fail visibly rather than opening with a
more permissive fallback.

## Terminal input and ownership

The GPUI view does not own a second shell parser. `shellfox-runtime` owns the
single Ghostty parser and PTY on a dedicated thread. Its commands and events are
bounded. The view accepts coalesced authoritative `ScreenFrame` state and credits
only the smaller of accepted-frame and delivered-byte sequences for its current
lease. This credit means bounded UI-state acceptance, not GPU paint completion.
Transitional clients that parse bytes have a different consumption boundary.

The runtime encodes key, paste and mouse commands against actual terminal modes
through Ghostty's public APIs. Printable native commits and physical special
keys stay separate, including preedit. Ctrl+C remains terminal input. Copy is
Ctrl+Shift+C on Windows/Linux and Cmd+C on macOS; paste is Ctrl+Shift+V or Cmd+V.
Ghostty rejects unsafe unbracketed multiline paste and the UI reports the error.

Drag to select. Double-click selects a word; triple-click selects a line.
Shift overrides terminal mouse reporting for local selection. Ghostty supplies
selection text and OSC 8 targets. Ctrl-click on Windows/Linux or Cmd-click on
macOS opens only parsed HTTP(S) URLs without embedded credentials or control
characters. Unsafe OSC 8 metadata never falls back to opening its label. Plain
text URL recognition currently works within a visible row, not across wraps.
Selection dragging does not auto-scroll beyond the viewport.

Mouse reporting uses Ghostty's public mouse encoder, never handwritten escape
sequences. Unsupported extra buttons are not invented. Wheel events go to the
application when mouse tracking is enabled, otherwise to Ghostty scrollback.
Canvas bounds, not whole-window dimensions, determine resize and pointer cells.
The grid is bounded by the runtime's maximum cell count.

The desktop starts one asynchronous `HostEndpoint` and talks through `HostClient` to the same `Service` owner used by native/local clients. It does not open a second metadata Host or SQLite connection. That service registers a generation before launch. Close/delete retains owners while cleanup is pending or failed, records stopped identities only after confirmed runtime closure, then commits metadata removal. Application shutdown awaits the endpoint's explicit Stop result.
A failed cleanup can be retried; it never deletes metadata and forgets a live
owner. Root exit drains final queued output before confirmed closure. Poisoned
callbacks stop the owned runtime rather than continuing with lost effects.

Windows process ownership still uses the trusted waiting supervisor, private
GO/READY handshake and non-breakaway kill-on-close Job. The parent retains the
original process and Job handles. There is no PID-name discovery or blanket
process kill. See [the PTY guide](../crates/shellfox-terminal/README.md) and
[wrapper build guide](../crates/ghostty-vt-sys/README.md) for platform and ABI details.

## Verification

Repeat scoped checks without forcing graphical initialization in host tests:

```powershell
cargo test --locked -p shellfox-desktop --bin shellfox
cargo clippy --locked -p shellfox-desktop --all-targets --no-deps -- -D warnings
node --test scripts/gpui-build.test.mjs scripts/gpui-assets.test.mjs
cargo test --locked -p shellfox-runtime --all-targets
cargo test --locked -p shellfox-terminal --features test-support --all-targets
```

Do not run the package-wide desktop test command while a user has the regular target executable loaded. Integration targets may try to rebuild it. The explicit `--bin` unit command and private `-o` link strategy have Windows locked-file regression tests.

The native acceptance helper `scripts/gpui-ui-acceptance.ps1` requires a private build path and a fresh isolated workspace. It records the exact executable hash, PID, birth, data directories and canonical source caption. Every action verifies those identities and `--ui-acceptance` flags. The flag disables activation/focus stealing; it does not mask caption differences. Capture uses only that HWND with PrintWindow. It does not capture the
whole desktop or use an unrelated window if capture fails. Its test project,
records and screenshots live under root `tmp/desktop-ui-acceptance`.

The current actor-backed native slice was exercised on Windows x86_64 in a fresh owned fixture, PID 39756, under `tmp/desktop-ui-acceptance/20261114-022424-8438`. Captures are named `v4-*`; they are diagnostic artifacts, not goldens.

- Native mouse/text messages completed the actual path-completion modal and created a persisted project session with the discovered PowerShell default.
- `ls` listed the owned project file. `node -e "console.log(6*7)"` printed 42 through the sole service parser and PTY.
- A separately built current CLI connected to that same data directory and listed the same session, cwd and tab. It did not open another Host.
- A second tab ran another real shell and printed its own marker. Returning to the first retained its prior output.
- Files read `proof.txt`. Native editing and Ctrl+S changed the actual owned file, checked separately from the UI. The accent preset changed the saved theme and existing terminal cursor live.
- A controlled Node TUI requested normal SGR mouse reporting. Native click and drag/release produced four reports and two releases in its real PTY log. The frontend used Ghostty's public encoder.
- Native double-click selected a known word. Ctrl+Shift+C copied exactly that controlled selection through the service and native clipboard callback.
- Explicit normal Quit waited for service Stop. The GUI exited and its private endpoint record disappeared. No process-name kill or forced cleanup was used.

Earlier standalone-view checks also exercised font family and size changes; those checks do not establish a complete current settings UI.

These are scoped native-message checks, not proof of physical keyboard layouts,
IME, screen readers, another OS, full feature parity or release performance.
No production store or installed Shellfox process was used. PIDs 9484 and 25904 are now user-occupied and must not receive automated input, capture, clipboard operations or cleanup. Preserve their data directories and earlier screenshots. Images from those windows are not parity goldens.

## Acceptance still missing

The existing Electron, React Native and headless implementations remain active.
This workspace is not a completed migration. Keep-awake is not represented by a fake UI toggle while its owner-backed integration is pending. Native profiles report their actual availability; the UI does not invent working WSL or elevation to match a fixture. The full settings presentation is present, but its additional host/device service adapter must be connected before those controls establish feature parity. Session and tab context workflows, per-tab file scopes, source-exact inline rename/delete behavior and volatile process-title/CWD/activity/error metadata still need acceptance. Strict pixel parity has not passed. Natural-source caption matching is not permission to mask OS focus, chrome, asset, font or layout differences. The existing final Electron reference records an active window; background, no-activate candidates must still fail any incompatible whole-window comparison. Native CJK/AltGr/dead-key/Option
composition, accessible terminal text, GPU/DPI coverage, sustained-output
benchmarks, full-screen TUI acceptance, window management, remote/SSH/files UI,
agent activity, automation, updates and packaging still need their own tests.

GPUI rendering uses per-cell shaping and background spans, not the required
optimized terminal text batching. Cursor blink/shape, images and complete IME
replacement/caret semantics remain incomplete. Linux/macOS/Windows arm64 desktop
acceptance and Android/iOS native integration are not established by a Windows
build. Shared host/runtime tests do not prove a mobile client or old React Native
client connects successfully. Recovery capability negotiation must not advertise
full terminal restoration when only a visible screen or formatted active buffer
is available.

No release, installation, update channel or production data migration was wired
by these desktop changes. The architecture and broader outstanding checks remain
in [the implementation plan](../IMPLEMENTATION_PLAN.md).
