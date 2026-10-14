# Administrator Windows PowerShell tabs

On Windows, right-click the `+` terminal button and choose **Administrator Windows PowerShell**. Windows requests UAC approval for that tab. Shellfox and its ordinary tabs keep their existing tokens. The administrator shell uses Windows PowerShell from the Windows system directory, not PowerShell 7 or a renderer-supplied executable.

The profile is an explicit tab choice. Discovery, settings, ordinary tab creation and session reopening do not elevate. Administrator Windows PowerShell cannot be saved as the default shell. Denying UAC produces a failed, closed tab and an error for the opening action. There is no external-window fallback.

Requires Windows 10 1809 or newer. Windows builds publish the x64 or arm64 helper as a self-contained .NET 8.0.31 application. Forge ships its complete directory at `resources/elevated-terminal`, outside ASAR. Development uses `tmp/terminal-native/win-<arch>`. The installed app does not compile the helper, download a runtime or extract executable libraries into a temporary directory.

## IPC and ownership

`PtyBackend` owns a `WindowsElevatedPty` through the existing terminal generation, replay, screen mirror, input rate and viewer-delivery paths. It registers cancellation before cwd validation or readiness waits. Service quit and update shutdown interrupt pending startups before draining operation queues. Closing a launching tab interrupts that exact generation before joining its session queue. A compact `Admin` badge follows the saved profile ID, so OSC titles and user renames cannot hide elevation.

1. The app starts the native launcher without elevation and gives it private inherited stdin/stdout pipes. Session environment and cwd are JSON data, not command-line text. CLR startup-hook and profiler variables are removed from the launcher's inherited environment and the merged shell environment. The native ConPTY constructor independently strips all `COR_`, `CORECLR_`, `COMPLUS_` and `DOTNET_` variables case-insensitively immediately before `CreateProcess`, including session overrides.
2. The launcher creates a random, single-instance Windows named pipe. `FILE_FLAG_FIRST_PIPE_INSTANCE` prevents name takeover; `PIPE_REJECT_REMOTE_CLIENTS` excludes remote connections. Its protected DACL grants only the initiating user, SYSTEM and Administrators. A medium mandatory-integrity label denies low-integrity writes.
3. `ShellExecute` with `runas` elevates only the helper. Before sending any launch data, the launcher verifies the connected client's PID against the live process handle returned by Windows. The helper verifies the pipe server's PID and birth against an open parent process handle. The endpoint name is not a bearer credential. The elevated client uses anonymous impersonation level, so the medium server cannot impersonate its elevated token.
4. The helper accepts one bounded launch frame and creates real ConPTY input/output pipes. It starts a fixed system PowerShell suspended, assigns it to a non-inheritable kill-on-close job with no breakaway permission, then resumes it. Null standard handles keep the launcher's redirected stdio out of the shell. VT output, input and resize travel through ConPTY, not an external terminal window.
5. A close or natural shell exit terminates the whole job. The helper waits for zero active job members and a signalled root handle before acknowledging exit. It drains ConPTY output while closing. Disconnect and parent death trigger the same cleanup. Watchdogs bound stalled resize, native close and launcher console writes. Process death closes the job handle but is not itself a successful cleanup acknowledgement.

Pipe messages have a four-byte length prefix and a 256 KiB limit checked before allocation. Host stdio uses bounded JSON lines. The native launcher forwards validated JSON as its original text, avoiding serializer escaping inflation. The host rejects oversized serialized input before reserving queue bytes or sending a frame; rejection does not disconnect the tab. Both sides accept terminal dimensions of 2 through 500 columns and rows. The helper rejects unknown or duplicate command fields; the host strictly validates replies. Input is capped at 64 KiB per request and 128 KiB outstanding, with at most 128 queued writes. Acknowledgements free exactly the matching bytes only after the helper writes and flushes them into the ConPTY input pipe. The backend also keeps its existing 256 KiB/s input limit. Output frames carry at most 4 KiB of base64-encoded PTY bytes; the host decodes UTF-8 across frame boundaries.

Cancellation removes the endpoint before any delayed UAC approval can receive a launch frame. Startup has deadlines. Cleanup errors retain ownership and prevent replacement or a claimed successful shutdown. Once a launch frame was sent, only authenticated job-empty evidence clears cleanup ownership. A helper crash may therefore leave an unconfirmed tab even though Windows has scheduled job termination. Restarting the app does not recover that PTY.

The trusted code and runtime directory must remain intact. This does not defend against an administrator, process injection into the same user's app, or intentional privileged commands typed after UAC approval. Release signing determines the publisher shown by UAC. The code does not install a privileged service or persistent elevation broker.

## Checks

- `pnpm typecheck`
- `pnpm exec vitest run src/main/terminal src/shared/terminal-schemas.test.ts src/renderer/terminal-workspace.test.tsx src/renderer/SettingsPanel.test.tsx`
- `pnpm test:packaging`
- `pnpm test:administrator`, no UAC. Publishes the helper and tests real ConPTY input/output, resize, exit code, descendant job cleanup, pipe identity, pipe-name collision and framing bounds.
- `pnpm test:administrator --profiler-dll tmp/review-administrator/profiler.dll`, optional harmless DLL regression proof when the review fixture exists. It sends unsanitized profiler variables directly into the native ConPTY constructor and verifies the DLL never loads. No UAC.
- `pnpm test:administrator --uac`, explicit live check. Run from a non-administrator terminal and approve UAC. It verifies an elevated PowerShell token inside ConPTY and that a fresh child of the host remains nonadmin.

Manual release checks still needed:

- Open normal tabs and browse the right-click menu. No UAC prompt should appear until choosing Administrator Windows PowerShell.
- Approve and deny UAC. Confirm shell input, paste, resize and screen replay in the app, with no separate PowerShell window.
- Close Shellfox while UAC is pending, then approve a delayed prompt. No shell should start.
- Exit PowerShell after starting a child; close a running administrator tab; terminate the app or helper. Confirm descendants disappear and missing cleanup evidence is reported rather than silently discarded.
- Run the packaged x64 and arm64 builds, including credential-based UAC with a different administrator account. Process tracking may remain unknown when the nonadmin host cannot inspect elevated command lines or executable paths. PTY I/O and job cleanup do not depend on that tracker.

Live x64 UAC approval passed: the embedded PowerShell reported an administrator token while a fresh host child stayed nonadmin. Resize, input/output, exit code and confirmed job cleanup passed. Windows x64 packaging, native/backend smoke and renderer/preload startup passed. The packaged UI administrator-tab attempt timed out waiting for UAC approval; ordinary tab I/O, resize and a nonadmin token passed, and the isolated processes cleaned up. Packaged administrator-tab I/O, UAC denial, delayed approval after quit, alternate-account credentials and packaged arm64 execution remain untested.
