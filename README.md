# Shellfox

Shellfox — terminal manager.

Shellfox keeps saved tasks on the left and real interactive terminals in the right workspace. Each workspace tab owns an independent `node-pty` shell, rendered by `@xterm/xterm` and sized by `@xterm/addon-fit`. It no longer opens Windows Terminal or GNOME Terminal windows for normal use.

This is an embedded-terminal preview. Windows x64 dependency/build/package checks have run locally, but full desktop acceptance is not complete. Linux, macOS, ARM64 and WSL remain release-unverified. See [platform support](docs/platform-support.md) and [verification](docs/verification.md).

## Using the workspace

- Create a session with an accessible folder. Multiple sessions can use the same folder.
- Select a session to show its current or first live tab. If it has no open terminal (for example after a restart), selecting it opens one fresh shell; commands are never restored. The session's environment variables apply to every new terminal in it.
- After a restart all saved sessions are listed, but no terminals open until you select a session.
- Right-click a session → Pin to top keeps it above the others (in pin order); Unpin returns it to the normal order. Archived sessions remember the pin; it applies again after Restore.
- The tab-strip plus creates another independent shell. Right-click it to choose a discovered profile.
- Switching sessions or Settings does not stop shells. Reattachment uses a bounded in-memory output replay; it is not a permanent terminal transcript.
- Closing a tab explicitly terminates its owned shell and verified descendants. Closure requires exit evidence, not just a sent signal. Do not use it on work you want to keep running.
- Archive moves a saved task into history without stopping shells or agents. Embedded archived sessions can be restored; retired external sessions remain read-only. Rename and agent-rule settings are available.

Terminals are app-owned. Quitting with live shells shows a native confirmation and closes them if confirmed. Cancel keeps the app running. Failed termination or missing exit evidence keeps the manager open for a retry. Closing or restarting the manager does not preserve shells. On startup, former embedded live tabs become closed records; commands and screen output are not restored. This replaces the previous external-terminal survival behavior.

Saved external-window sessions remain legacy history metadata. The embedded runtime does not adopt, watch, focus or kill those old windows. Old titles, registrations and window bindings are not evidence of an owned embedded shell.

## Shells and WSL

Windows discovers PowerShell 7 at its standard installation path, then falls back to Windows PowerShell. Linux and macOS discover the user's login shell and available local fallbacks. Choose a discovered default in Settings; executable/command templates are not supported. Shell startup files still run for ordinary interactive profiles.

Installed WSL distributions appear as separate Windows profiles. Discovery now preflights each guest before permitting launch. This can boot distributions, with bounded concurrency and timeouts. Bash, env, Python 3, readable guest `/proc` and working pidfd signaling must pass; unavailable profiles remain listed in Settings, and the backend returns an explicit reason. The current UI labels them unavailable rather than displaying every per-profile reason. WSL tabs use guest Bash and guest process tracking, not `wsl.exe` ancestry as a substitute. Use an absolute guest folder such as `/home/me/project`, or a Windows drive path that the selected distribution can translate with `wslpath`. Initial session creation uses the saved default profile; Add tab can select another profile.

Linux local profiles resolve Python from Settings → Python, then `/usr/bin/python3`, then PATH. The chosen interpreter must pass readable `/proc` and working `os.pidfd_open` / `signal.pidfd_send_signal` preflight before launch. WSL requires the same guest APIs plus Bash/env. A kernel version alone is not proof; preflight checks usable signaling and process access.

Linux/WSL close freezes and enumerates the owned PTY session to a fixed point, signals held pidfds for its root and descendants, and confirms every exit. Only the original live stopped session leader authorizes new session members. Previously proven detached/orphan identities need independent exact-birth verification; unknown ownership causes refusal. Shell exit alone does not discard pending descendant cleanup. Failed cleanup blocks replacement and quitting, retains ownership and resumes surviving processes stopped by that invocation. A killed/crashed cleanup helper cannot guarantee restoration.

If prerequisites disappear after launch, or members become elevated/inaccessible or exceed the 256-member bound, close can fail honestly. The manager does not kill `wsl.exe` first or use numeric-PID fallback. Exit the shell manually or restore the prerequisites. Actual Linux pidfd/tree safety fixtures and a target-native installed Debian x64 UI/CLI smoke passed inside WSL Debian. This does not establish Ubuntu-flavour physical desktop, AppArmor-policy or ARM64 acceptance.

## Development

Use Node 24 and the pnpm version specified in package.json, running natively on the target OS and architecture.

```sh
pnpm install --frozen-lockfile
pnpm typecheck
pnpm test:unit
pnpm build
pnpm test:storage
pnpm test:pty
pnpm start
```

`pnpm dev` builds and launches the app. It is not a hot-reload server. Normal install, build, package and make do not require .NET or an external terminal application.

Build prerequisites:

- Windows requires a supported Electron Windows version with ConPTY, practically Windows 10 1809 or newer, or Windows 11. PowerShell must be available. Source compilation additionally needs Python 3, Visual Studio C++ build tools, Windows SDK and Spectre-mitigated libraries for the target architecture.
- Linux requires Python 3, `make`, a C/C++ compiler and Electron's graphical/runtime libraries. `node-pty` 1.1.0 has no Linux prebuild, so it compiles against the pinned Electron. Debian packaging also needs `dpkg` and `fakeroot`. A graphical session is needed for the desktop app; headless Electron tests need Xvfb.
- macOS builds always need Xcode Command Line Tools and the macOS SDK to compile the native tracking/lifecycle helpers. Python 3 is needed only if native addons must compile from source, not by the installed Darwin terminal runtime. The helper build targets macOS 13 or newer; this is not a broader Electron compatibility claim.

The pinned `node-pty` uses Node-API prebuilds on Windows/macOS where supplied. `pnpm rebuild` copies only the host platform/architecture's prebuild, or rebuilds from source against the pinned Electron when absent. Set `SHELLFOX_BUILD_FROM_SOURCE=1` to force compilation. Both SQLite and PTY native modules must load in Electron before build continues. No host-Node load is used as proof of Electron compatibility. Native dependency install scripts are disabled for these two modules; run build or rebuild before native tests.

This host's forced source rebuild failed because Visual Studio's Spectre libraries are missing. The default validated Windows x64 prebuild path works. End users should not need developer Node, .NET or compilers, but clean-machine deployment is unverified.

## Updating and releases

Run `shellfox update` to download and install the latest published release (`shellfox update --check` only reports). Linux uses the `.deb` (`sudo apt-get install`), macOS replaces `Shellfox.app`, Windows runs `ShellfoxSetup.exe`; zip/loose installs print the releases page instead. Installed builds check for updates at startup and hourly. The sidebar offers Download update with progress, then Install and restart with confirmation before closing terminals. Hiding a notice lasts until the next start. See [in-app updates](docs/self-update.md) for supported installations and verification details.

To publish a release, set `version` in package.json and push a matching tag (`git tag v0.2.0 && git push origin v0.2.0`). The Release workflow packages Linux (x64, arm64), Windows (x64; runs on ARM via emulation) and macOS (x64, arm64) and attaches the installers to a GitHub Release.

## Packaging

```sh
pnpm package
pnpm test:packaged
pnpm test:packaged:startup
pnpm make
```

Run on the target OS with target-native x64 or arm64 Node. Do not reuse another platform's `node_modules` or relabel a native binary. Universal macOS and cross-compilation are not configured.

| Host target | Configured artifacts |
| --- | --- |
| Windows x64 | Loose app, ZIP, unsigned Squirrel installer |
| Windows arm64 | Loose app and ZIP; no Squirrel installer claim |
| Linux x64/arm64 | Loose app, ZIP and DEB |
| macOS x64/arm64 | App bundle and ZIP, ad-hoc signed previews or optional Developer ID signature; unnotarized |

Isolated test launches keep their windows hidden, non-focusable and off the taskbar by default. Rendering stays active for UI automation. Set `SHELLFOX_TEST_VISIBLE=1` only when you want to watch or debug test windows. Normal app launches are unchanged.

Terminal tabs can be renamed by double-click, F2 or their context menu. Drag tabs left or right within a session to reorder them, or use Alt+Arrow. Names and order persist after restart.

Outputs live under `tmp/build/`, `tmp/build-test/` and `tmp/packages/`. For Windows x64 the runnable package is `tmp/packages/Shellfox-win32-x64/Shellfox.exe`. Makers produce artifacts under `tmp/packages/make/`. Building an installer does not install it or verify its lifecycle.

Forge stages bundled application code and the native runtime modules only. SQLite's matching prebuild or rebuilt addon is retained. The entire staged `node-pty` runtime is outside ASAR, including child-process JS agents, `.node` addons, ConPTY DLLs/OpenConsole, winpty helpers and the macOS executable `spawn-helper`. No legacy .NET helper or registration shell scripts ship with the embedded package. `test:packaged` checks inventory and exercises the actual packaged Electron, SQLite and native PTY. It also runs the stamped backend source against that package's native modules, without a fake factory, to test profile readiness, replay and confirmed close. These fixtures do not exercise terminal IPC/UI or the quit dialog. `test:packaged:startup` launches the production renderer/preload with isolated data and no shells.

macOS helper sources compile at build time with `xcrun --sdk macosx clang`, explicit native architecture and strict warnings. Development binaries live under `tmp/terminal-native/darwin-<arch>/`; packaged binaries live outside ASAR under `Contents/Resources/terminal-native/`. Runtime preflight never invokes a compiler or falls back to development paths. Signing includes native resources and node-pty's executable helper before sealing the app. Preview signing is ad-hoc. Set `SHELLFOX_MAC_SIGN_IDENTITY` to an installed Developer ID Application identity and optionally `SHELLFOX_MAC_SIGN_KEYCHAIN` for certificate signing; missing certificates fail packaging. Notarization, Gatekeeper and native Darwin launch/close acceptance have not run. The native supervisor and TypeScript lifecycle/profile integration are implemented. A compatible shipped-helper preflight enables macOS profiles; missing or incompatible binaries remain unavailable with a dependency reason. Actual native Darwin acceptance is still unverified on this Windows host. See [macOS build details](docs/macos.md).

The manual [CI workflow](.github/workflows/embedded-packages.yml) configures six native targets. It has not been run for this rewrite. [Ubuntu instructions](docs/ubuntu.md) cover Linux dependencies and desktop testing.

## CLI

After build:

```sh
pnpm exec electron . --new-session --cwd "C:\work\project"
```

On Ubuntu/Debian, install with `sudo apt install ./shellfox_*.deb`; `/usr/bin/shellfox` is included. `shellfox` opens the app and `shellfox start .` selects a session for your current folder. Linux Settings also installs per-user right-click entries for Files/Nautilus, Nemo, Dolphin, Thunar and Caja. See [Ubuntu install and usage](docs/ubuntu.md), including Python path and sandbox details.

For Windows or loose/from-source Linux builds, enable `shellfox start <path>` in Settings to install the terminal command. Run `shellfox start .` or `shellfox start "C:\folder with spaces"` from cmd, PowerShell, Git Bash or WSL. Paths resolve against the caller's working directory. The command returns without waiting for the app to close. Run `shellfox --help` for usage. New external terminals pick up the user PATH change; new embedded terminals receive it immediately.

On Windows, enabling the CLI also prepends the Windows launcher directory in each registered WSL distribution's default user's shell startup files. This makes bare `shellfox` resolve to the Windows launcher before an older `/usr/bin/shellfox`. Bash uses `~/.bashrc` and the first existing login file among `~/.bash_profile`, `~/.bash_login`, and `~/.profile`, creating `.profile` only if none exists. Zsh uses `~/.zprofile`, `~/.zshrc`, and `~/.zlogin` when zsh or its startup files are present. Shellfox adds marked blocks, preserves other content, and removes only its blocks when disabled. It does not use sudo or change the Linux launcher.

`shellfox start .` from WSL opens a session in the running Windows window. PowerShell tabs use the Windows drive or `\\wsl.localhost\<distro>\...` path; WSL tabs use the guest path. The Windows app has no Electron menu bar. Linux behavior is unchanged.

Restart existing WSL shells after enabling or disabling. To apply enabling in place, reload the startup file Shellfox updated, then run `hash -r` in Bash or `rehash` in zsh. A hash reset alone does not change an existing shell's PATH. After disabling, restart to remove the injected PATH entry. Check with `type -a shellfox`.

WSL integration is best-effort and can boot registered distributions. It requires guest Python 3, `wslpath`, access to the Windows bin directory, and Windows interoperability to launch the app. Missing WSL or a failed guest update does not disable the Windows CLI. Symlinked, hard-linked, inaccessible, non-regular, or other-user startup files and malformed Shellfox blocks are left untouched. Only each distribution's default user is configured, not root or other users unless they are the default. Other shells, custom `ZDOTDIR`, shells started without startup files, aliases/functions named `shellfox`, and later PATH overrides are not covered. New distributions need the setting reapplied. CLI installed status checks Windows files and PATH, not every guest's startup configuration.

Tests with a custom CLI bin, registry key, or registry runner skip WSL by default. `ShellfoxCliOptions.wsl` accepts an explicit injected `run` and/or an absolute guest `home` for isolated testing; `wsl: false` disables guest updates. Do not opt test builds into a real guest HOME.

The older `--new-session --cwd` flags are also supported, with both flags required. Each invocation creates a separate saved session, even in the same folder. Request UUIDs prevent duplicate delivery. Early requests wait for initialization. Settings also offers `Open in Shellfox` for Explorer folder and background menus. On Windows 11, use Show more options. Registry changes affect only app-owned verbs.

## Storage, tracking and security

SQLite lives in `<Electron appData>/Shellfox/manager.sqlite3`. The main process is its only writer. On the first renamed startup, if that database is absent, Shellfox copies the previous app's database and settings without deleting the old directory. SQLite backup includes committed WAL data; `legacy-backup` keeps raw DB/WAL/SHM recovery copies. Existing Shellfox databases are never overwritten. Renderer preferences migrate their old storage keys on read. Saved Explorer and CLI opt-ins are reapplied with the new executable. Squirrel now has a new installed identity and install path; this is a separate installation, not an in-place update of the old package. Migrations retain legacy history and persist embedded tab/profile/exit metadata. Terminal output and process command lines are not stored in SQLite or manager snapshots. Scrollback and replay are bounded and disappear at app exit. Shell exit is forwarded even if SQLite saving fails; runtime closure remains authoritative, a storage error is surfaced and metadata reconciliation retries separately.

Counts mean matched descendant process identities, not logical tasks, input-wait detection or command success. Windows local tracking uses PowerShell/CIM, Linux uses `/proc`, and macOS uses a read-only libproc/numeric-sysctl snapshot helper, with helper self-preflight before advertising tracking. WSL tracking runs inside the selected distro. Poll failures and inaccessible identity evidence stay unknown. Very short-lived, detached, elevated, remote, container or multiplexer activity is not guaranteed. The displayed folder is the starting/default folder, not live shell CWD. Local folder validation checks accessibility and the resolved target, rejects unsupported remote paths, and accepts supported local symlinks.

The renderer is sandboxed with no Node integration or raw IPC. Preload exposes validated terminal operations; main checks sender, generation and ownership before input, resize or close. VT output goes to xterm rather than HTML. OSC clipboard and link activation are disabled; no clipboard or web-links addon is installed. A shell's exit code is not arbitrary command-status detection.

Attachment to existing external terminals, split panes, arbitrary command templates, automatic command restoration and persistent detached terminals are not implemented. Legacy .NET and external-window tests remain in the repository for regression work only; their historical successes are not acceptance evidence for this runtime.
