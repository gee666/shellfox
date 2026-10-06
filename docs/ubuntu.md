# Ubuntu embedded-terminal builds

The current Linux runtime embeds an interactive shell in the right workspace using node-pty and xterm. It no longer requires GNOME Terminal, a registered Bash rcfile or a self-contained .NET helper. Older `0.1.0` DEB artifacts in scratch directories predate this rewrite and must not be used as evidence for it.

## Build on the target host

Use a glibc-based Ubuntu desktop, Node 24 and pnpm 12.8.1 with matching x64 or arm64 architecture. Install development dependencies only with the machine owner's consent. Typical Ubuntu 24.04 prerequisites are Python 3, make/g++, dpkg, fakeroot, libgtk-3-0, libnss3, libxss1, libgbm1, libnotify4, libxtst6, libasound2t64 and xdg-utils. Electron's exact runtime dependencies vary by distribution. For headless tests also provide Xvfb and session D-Bus.

```sh
pnpm install --frozen-lockfile
pnpm typecheck
pnpm test:unit
pnpm build
pnpm test:storage
pnpm test:pty
pnpm package
pnpm test:packaged
pnpm test:packaged:startup
pnpm make
```

node-pty 1.1.0 does not ship Linux prebuilds. Build compiles it against the pinned Electron and verifies native loading in Electron alongside SQLite. No .NET build runs. Do not copy Windows/macOS node_modules or reuse their build/Release files. `node scripts/ubuntu-stage.mjs` can copy source into `tmp/ubuntu-source`; install fresh dependencies there on Linux. No prepublished .NET helper is required.

Headless Electron commands need a graphical wrapper, for example:

```sh
dbus-run-session -- xvfb-run -a pnpm test:storage
dbus-run-session -- xvfb-run -a pnpm test:pty
```

The package is `tmp/packages/Shellfox-linux-<arch>/shellfox`. Makers produce ZIPs and `tmp/packages/make/deb/<arch>/*.deb`. DEB dependencies no longer include GNOME Terminal. A shell and working directory must still be available. Electron's Chromium sandbox also needs a working host namespace configuration or its correctly installed root-owned mode-4755 chrome-sandbox helper. The disposable CI runners configure that development/loose-package helper and permit private user/PID namespaces for the isolated PID/SID-reuse fixture. The Ubuntu runner's unprivileged-userns AppArmor restriction is relaxed only in CI; production launch does not need the fixture's namespace policy change. CI never disables Chromium sandboxing; this is not clean-machine installation acceptance. Do not change host sandbox permissions or policies without administrator approval. The package does not install user profiles, registration scripts, file-manager verbs or default-terminal settings.

Python 3 is also a runtime prerequisite on Linux, not just a compiler dependency. Launch preflight requires `/usr/bin/python3`, readable `/proc` and working pidfd-open/signal APIs under the current user. Missing kernel support, seccomp restrictions or inaccessible process evidence disable profiles with a reason. Build success does not bypass this gate. Unix shell aliases are canonicalized for executable identity.

Close uses held pidfds to freeze and enumerate the owned session and verify root/descendant exit. Unknown ownership, inaccessible/elevated members, lost prerequisites or the 256-member bound can block cleanup and quitting. Root exit alone does not authorize a new shell while cleanup remains unconfirmed. Failed invocations resume still-live members they stopped; external destruction of the cleanup helper cannot guarantee restoration.

## Desktop installation and acceptance

After separate permission to install a newly built artifact on a disposable target desktop:

```sh
sudo apt install ./shellfox_<version>_<debian-architecture>.deb
shellfox
```

Use the actual artifact filename. Do not launch the app with sudo, disable Chromium's sandbox to hide a configuration error, or bypass dependency failures. Capture the exact error instead.

Create sessions and independent tabs, run input/output and a full-screen program, resize the workspace, switch sessions/Settings, settle a live task, and check quit cancellation and confirmed termination. Verify restart shows history without replaying commands. Linux process visibility failures must show unknown, not an invented healthy state.

Linux/Ubuntu desktop acceptance and clean installation have not run for this rewrite. Actual Linux kernel pidfd/tree tests did run inside WSL Debian, including unrelated-session protection, stale identities, reused session IDs and failed-close resume. They are not a native Linux node-pty/Electron package test. Old WSL Debian SQLite/helper smokes and GNOME registration tests are historical only. The manual native-target [CI workflow](../.github/workflows/embedded-packages.yml) is configured but unexecuted; its build artifacts will not by themselves prove desktop installation or complete UI behavior.
