# Electron and GPUI parity

The requested target is literal interface and functional parity with the current Electron application. Similar colors, roughly matching controls, a runnable shell, or screenshots of a different design do not meet that target.

## Current evidence

The canonical Windows reference is:

```text
tests/visual-parity/baselines/windows-x64-dpi96/electron-reference-final/
```

It contains 25 states from the actual Electron `BrowserWindow`, built from this worktree's unchanged committed application sources. Each state has:

- `.window.png`, the complete owned native window, including title, icon, minimize/maximize/close controls and frame.
- `.client.png`, the native client area from the same window capture.
- `.renderer.png`, an independent Playwright screenshot of the real renderer.
- `.geometry.json`, measured element rectangles, computed styles, CSS variables and exact SVG markup.
- `.window.json`, native PID/birth/HWND, DPI, focus and client/window/DWM bounds.

`manifest.json` records the Electron/Chromium versions, build source digest, Git revision, display properties, image hashes and confirmed cleanup. `interactions.json` records actual fixture actions and resulting host snapshots or disk-byte assertions.

A second independent Electron launch produced `electron-reference-repeat/`. The strict comparison in `tests/visual-parity/reports/final-repeatability-hash-checked/comparison.json` passes all **50 of 50** client/native comparisons with **zero differing pixels**. No presentation changes, exclusions, masks, antialias exemptions or resizing were used.

This proves repeatability of these Windows reference states on this machine. It does not establish GPUI parity or complete application acceptance.

A separately launched and closed GPUI artifact is recorded in `gpui-empty-reference-1/`. Its empty window differs from Electron. The strict comparator also fails every missing candidate state rather than silently skipping it. The candidate's executable and Cargo lock hashes identify the captured artifact; concurrent GPUI source work may be newer than that binary.

Earlier `electron-first-reference-*`, `electron-full-reference-*` and `electron-repro-reference-1` directories are preliminary reference/debug evidence, not the canonical acceptance set. One preliminary repeat had a different native frame paint color. That difference was not masked or tolerated. The final reference and independent final repeat match the entire native frame exactly.

## Recorded states

| Group | States |
|---|---|
| Empty/window chrome | `01-empty` |
| New session | `02-new-session-dialog`, `03-new-session-filled` |
| Settings | `04-settings-general`, `settings-terminals`, `settings-agents`, `settings-remote-access`, `settings-ssh` |
| Session controls | `05-session-context-menu`, `06-session-rename`, `08-session-environment`, `09-session-pinned` |
| Real terminal/tabs | `07-session-terminal`, `10-terminal-profile-menu`, `11-terminal-context-menu`, `12-terminal-rename`, `13-two-tabs` |
| Files/editor | `14-files-browser`, `15-file-editor`, `16-file-editor-dirty`, `17-file-editor-saved` |
| Destructive action | `18-delete-session-confirmation` |
| Status | `19-settled-status`, `20-error-status-fixture`, `21-error-context-menu` |

The error states inject valid monitor-error metadata through the existing test-only repository/service hook. They exercise the real renderer and real clear-error action. They do not prove that an actual process monitor failed. Running/busy animation, file-transfer dialogs, SSH edit/connect dialogs, update/quit confirmation, and further native-window interaction states still need controlled visual fixtures.

## Exact reference bundle

`electron-reference-final/reference/inventory.json` maps every copied asset to `originalPath`, `artifactPath` and byte SHA-256. The bundle includes the production CSS, SVG icon definitions, ASCII fox, terminal font configuration, native-window setup and application icon files. Historical `.ts`/`.tsx` files have `.source` appended. They remain exact reference bytes, not extra TypeScript compilation units. Do not change application tsconfigs to accommodate reference snapshots.

The same inventory records the Windows build, installed font families and SHA-256 of Segoe UI/Consolas font files. Cascadia Code, Consolas and DejaVu Sans Mono are installed on this capture machine. A different font installation cannot be treated as equivalent without native evidence.

Useful measured values at the recorded viewport:

| Element | Actual Electron value |
|---|---|
| Client area | 1200 × 760 physical/CSS pixels, 96 DPI, zoom 1 |
| Native outer window | 1216 × 799, positioned at 64,64 |
| Client offset within outer window | 8,31 |
| UI font | 13px Segoe UI through the production system-font stack |
| Session sidebar | 220px plus a separate 4px resizer; workspace begins at x224 |
| Sidebar toolbar | 32px |
| Session row | 36px, selected left border 2px |
| Session text | title 13px/14px, subtitle 11px/14px |
| Icon button | 28 × 28 |
| Terminal strip/tab | 30px, tab min-width 90px/max-width 180px, selected bottom border 2px |
| Terminal text | 14px Cascadia Code, then Consolas/DejaVu Sans Mono/monospace |
| Terminal padding | top/left 8px, right 2px |
| File sidebar | x780, width420px including its 4px left resizer |
| Settings header/categories | 36px / 34px; category gap12px, horizontal padding20px |
| Settings body | padding22px 24px 36px, max-width720px |
| New-session modal | x330,y303,width540,height154; radius4px |
| Modal header | x331,y304,width538,height45; padding8px 12px 8px 16px |
| Directory form | x331,y349,width538,height107; padding12px |
| Directory path line | x343,y388,width514,height36; gap6px; font13px/22px Cascadia Code |

Palette values come from actual computed styles, not screenshot approximations: background `#111016`, foreground `#e5e4eb`, muted `#a5a4ab`, border `#2c2a3a`, sidebar `#15141b`, panel `#18171f`, surface `#23212d`, hover `#312e3f`, input background `#121117`, input border `#3b384d`, accent `#ec4899`.

Electron has a real native Windows titlebar with the fox icon and `Shellfox v0.9.13`. The empty workspace has the production ASCII fox watermark. Replacing either with text, different icons or custom approximate chrome is a visible mismatch.

## Safe reproduction

Use the normal test build. The existing Go/C# prerequisites must be available; do not bypass ownership or private-permission checks.

```text
SHELLFOX_GO=P:/workshop/Windows_Worshop/oira666/shellfox-ghostti/tmp/toolchains/go1.27.3/go/bin/go.exe node scripts/build.mjs --test
node scripts/electron-parity-capture.mjs UNIQUE-ELECTRON-LABEL
node scripts/electron-parity-reference.mjs tests/visual-parity/baselines/windows-x64-dpi96/UNIQUE-ELECTRON-LABEL
```

The recorder refuses changed committed Electron source or a stale standard build stamp. It uses the existing test-build launch gate with both `--test-user-data` and an explicit `real` backend. A fresh UUID directory beneath project `tmp/` isolates the single-instance lock, host endpoints, SQLite database, caches and settings. It never attaches to an installed or existing production host.

The environment is allowlisted. HOME, USERPROFILE, AppData, XDG directories and temp paths are private to the run. Real PowerShell launches use the existing actual backend with the safety-only `-NoProfile` argument added through its test hook. A fixed prompt and fixed output provide terminal fixtures. No authentication files, profiles, account sessions or parent credential variables are copied. Remote settings stay disabled and no login/connect action is performed. Renderer outbound requests are blocked.

Only files under `tests/visual-parity/fixtures/project` are edited. The save case asserts actual UTF-8 disk bytes and restores the fixture. The source app directs automatic CLI/Explorer test registrations into a unique `HKCU/Software/Shellfox/Tests/<isolated-data-hash>` namespace. The recorder removes only that exact namespace after shutdown. Production PATH and Explorer registration keys are not used.

Cleanup closes each owned embedded terminal using its exact tab/generation, then closes only the launched Electron application. Failure retains the isolated run and reports it rather than killing arbitrary processes. Successful final runs record cleanup confirmation and remove their temporary data.

## Native capture and candidate checks

```text
node scripts/electron-parity-candidate.mjs UNIQUE-GPUI-LABEL
node scripts/electron-parity-compare.mjs BASELINE-DIR CANDIDATE-DIR UNIQUE-REPORT-LABEL
```

The candidate command launches a new worktree `target/debug/shellfox.exe` with a fresh explicit data directory. It currently captures only the empty state. It refuses unexpected nonempty session state. It does not attach to another GPUI instance and cannot claim the remaining states passed.

The PowerShell helper opens the exact requested process, verifies its birth FILETIME, executable path and HWND owner, and checks identity again around capture. PID 9484 and PID 25904 are explicitly protected before native access. The candidate exercises the wrong-birth guard and records its rejection. Resizing and `WM_CLOSE` target only the newly owned, verified HWND. There is no global keyboard/mouse injection, foreground-window guessing, numeric-PID kill fallback or screen-crop fallback.

Images use `PrintWindow` on that HWND. The full native bitmap is retained; the client image uses the measured client origin. The comparator checks platform, architecture, DPI, focus state, recorded screenshot hashes, complete state coverage, dimensions and every decoded RGBA channel. Missing states, a one-channel one-pixel change, or a native frame difference fail. Diff images highlight differences but never alter the images being judged.

## Functional acceptance

`tests/visual-parity/interaction-matrix.json` maps controls and actions to the existing Playwright suites and required effects. The recorder additionally asserts real session/tab creation, renames, pinning, editor disk writes, cancel-delete retention, settlement and error clearing.

Those checks are not a substitute for the existing terminal, security, delivery/recovery, files, remote, SSH, CLI, lifecycle and update suites. In particular, screenshot parity cannot prove PTY cleanup, generation/controlEpoch enforcement, authorization, credential handling, native dialogs or real remote operations. A GPUI implementation must pass equivalent assertions for the same actions, not hide missing controls or return fake success.

Native macOS and Linux baselines and functional runs are blocked on this Windows host. Their window controls, fonts, decoration/compositor behavior and DPI must be recorded and compared on their own platforms. Do not copy Windows goldens or infer acceptance from shared source. Mobile behavior remains separately subject to its existing native/React Native acceptance tests.

## Focused checks

```text
node --test tests/visual-parity/*.test.mjs
```

These test exact PNG handling, one-pixel/dimension rejection, screenshot/source provenance, inert historical source files, protected-PID rejection and the recorded independent repeatability result. Full application parity remains unaccepted until all required GPUI states and functional cases pass on every supported desktop platform. No masks or dynamic-content exclusions have user approval.
