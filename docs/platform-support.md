# Platform support

The embedded runtime uses one real PTY per right-workspace tab. It does not route tabs into Windows Terminal or GNOME Terminal. App-owned shells and owned descendant cleanup end through confirmed close/quit; restart retains task history, not live shells.

## Implementation and evidence

| Target | Implementation | Configured artifacts | Host evidence |
| --- | --- | --- | --- |
| Windows x64 | PowerShell, bundled ConPTY DLL/OpenConsole and CIM tracking | Loose app, ZIP, Squirrel | Windows-native dependency/build/package checks have passed. Final review rebuild is recorded in verification.md. Full desktop/installer acceptance incomplete. |
| Windows arm64 | Same profile/backend code with arm64 native addons | Loose app, ZIP | Not run |
| Linux x64/arm64 | Canonical login shell, /proc tracking and Python pidfd tree cleanup | Loose app, ZIP, DEB | Kernel/tree safety fixtures passed inside WSL Debian, not a native Linux Electron package |
| macOS x64/arm64 | Read-only libproc/numeric-sysctl snapshot helper; per-tab lifecycle supervisor integration | App bundle, ZIP, ad-hoc or optional certificate signature | No macOS SDK on this host. Native compile/sign/launch/close unverified |
| WSL on Windows | Per-distro Bash, guest markers, /proc identities and pidfd cleanup | Windows host package | Debian kernel tests only. WSL UI/package acceptance unverified |

The six-target CI configuration is not evidence that all targets have launched or shut down correctly. Linux is a glibc desktop target, not Alpine/musl support. Builds require target-native Node/OS/architecture. No universal binaries or cross-staged addons are supported.

## Prerequisites and preflight

Normal startup/build needs no .NET SDK/runtime, Windows Terminal or GNOME Terminal. See [README](../README.md#development) and [macOS builds](macos.md) for compiler requirements.

Windows needs ConPTY and a discovered PowerShell. Backend requests the packaged ConPTY DLL path rather than the asynchronous AttachConsole/list-PID close path.

Linux local launch requires canonical shell identity, /usr/bin/python3, readable /proc and a successful actual pidfd-open/signal preflight. Python remains an installed runtime prerequisite; DEBs declare it. Kernel version or build success alone cannot override missing capabilities.

WSL discovery checks Bash, env, Python 3, /proc and usable pidfd signaling before enabling a distro. Preflight can start distributions. Checks have bounded concurrency and per-command timeouts. Unavailable profiles cannot launch and retain explicit reasons in the API. The current Settings list labels them unavailable; not every per-profile reason is rendered. Windows-drive CWD translation also needs wslpath. Neither host wsl.exe ancestry nor a discovered distro name proves guest shell ownership.

The native macOS supervisor, TypeScript client/backend lifecycle wiring and profile preflight are implemented. Profiles become available only after a compatible shipped-supervisor preflight succeeds; missing binaries are a dependency failure, not an unsupported placeholder. macOS needs both compiled helpers in the exact packaged resource directory or development artifact path. Self-preflight must establish readiness before launch/tracking capabilities are advertised. The snapshot helper alone is read-only and cannot supply safe descendant termination. A missing, incompatible or failing supervisor leaves profiles unavailable rather than enabling a numeric-PID fallback. The installed Darwin runtime does not need Python or a compiler. Native macOS execution remains unverified even when implementation and build configuration are present.

## Descendant shutdown and limits

Linux/WSL termination holds pidfds, freezes/enumerates to a fixed point and independently verifies exits for the root and owned descendants. New session members require the original live stopped immutable leader. Cached orphan session IDs never authorize ownership. Previously proven detached/orphan identities require fresh exact-birth confirmation; unobserved detached activity is not guaranteed.

A shell exit cannot erase pending descendant cleanup. Failed cleanup retains ownership, blocks silent replacement and prevents quitting. The failed invocation resumes survivors it stopped, including normal deadline interruption. External SIGKILL/crash of the cleanup helper cannot guarantee restoration. Missing prerequisites after launch, elevated/inaccessible members, unverifiable ownership or more than 256 members can still cause honest refusal.

The Darwin lifecycle supervisor is separate from read-only snapshots. It retains the owned session leader and uses same-session group guardians and private same-user control; main does not signal a target by numeric PID. The backend authenticates and tracks the real foreground shell child. Confirmed CLOSE waits for native wrapper exit; timeout never kills that wrapper. Crashes/SIGKILL retain uncertain cleanup ownership. Detached new sessions are outside the protocol and are not swept. Real job-control and shutdown behavior must still be verified on Darwin before release.

## Agent detection

Pi, Claude, Codex and OpenCode each have one switch for native executables and Node/Bun launches. Detection trusts an exact executable basename, OS process name or rewritten argv0 first. Custom name rules use the same matching. Exact executable-path rules match only that path, never a name or title. Names and paths are case-insensitive on Windows and case-sensitive on Unix.

When no agent name is recognized, bundled rules check the Node/Bun script slot against package suffixes. This works across runtime versions and install directories. Bun direct scripts and `bun run <script-path>` are supported, not package.json task names, arbitrary wrappers or prompt arguments. Pi titles no longer require installer/shim provenance.

Linux/WSL collectors expose proc comm and argv0; macOS exposes libproc names and argv0. Windows CIM exposes the executable Name and original command line, not Node's later console title. Windows title-only Node detection is therefore unavailable; native names and script paths still work. Truncated OS names must match exactly; argv0 can supply the full name. Native macOS execution remains unverified.

Stored bundled launcher rules migrate to the unified switches. An existing per-agent toggle wins over the old Native agents toggle, including when disabled. If a per-agent rule is absent but the untouched native rule covered that agent, its old native toggle supplies the missing switch. Customized legacy rules retain their constraints and toggle under custom IDs; unrelated custom rules are untouched. Removed rules stay removed when no old native coverage exists. Detection changes do not grant tab ownership or cleanup authority.

## Workspace behavior

- Activation selects a live tab or opens one fresh shell only when no live/launching shell or unresolved cleanup remains.
- Add tab owns another independent PTY, not an HWND-routed append.
- Session/Settings switches retain shells while the app runs. Closed/stale generations cannot receive input.
- Native quit confirmation is independent of renderer confirmation. Cancel keeps shells running; failed cleanup keeps the manager open.
- Settlement is history bookkeeping, never process termination.
- Legacy external-window records remain history metadata; the runtime does not adopt, watch or kill them.
- Interactive shell exit status is not arbitrary command-status detection.
- Inaccessible tracking evidence stays unknown. Very short-lived, detached, elevated, remote/container or multiplexer activity is not guaranteed.

No existing-terminal attachment, split panes, daemon-backed survival, SSH-specific profiles, command restoration, arbitrary executable templates or Explorer/Finder/Nautilus integration is advertised. Historical Windows Terminal routing and GNOME/.NET tests describe the retired backend, not embedded acceptance.
