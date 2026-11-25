# Updates

Shellfox has two updaters. Desktop installs use the one in the desktop app (`crates/shellfox-desktop/src/update/`). Headless packages use `shellfox update` in the CLI (`crates/shellfox-cli/src/headless_update.rs`). Both read only the latest stable release of `gee666/shellfox` on GitHub.

## Desktop app

The app checks at startup and then every hour. A newer release shows a compact notice at the bottom of the sidebar: a download button, then a progress bar with the estimated time left, then **install and restart**.

With **Automatic updates** on (Settings > General, on by default), the app downloads the update in the background and installs it when you quit. Terminals keep running during the download. With it off, nothing downloads on its own, but the sidebar buttons still work.

**install and restart** opens a confirmation that defaults to Cancel. If shells are open, it says how many will close and that running commands stop. Cancel leaves the download ready and the shells running. External terminals are never closed.

The installer runs only after a clean quit, once the terminal host has stopped, because the update replaces the files the host runs from. A detached helper script checks the download again, waits for the app and its `shellfox-cli --desktop-host` process to exit, then installs. Quitting installs without relaunching; **install and restart** relaunches. If the install fails, the helper shows an error (on Linux only when `zenity` is installed), keeps `install.log` in the temporary `shellfox-update-*` folder, and reopens the old app if you asked for a restart.

Supported installs:

- Windows x64 installed with `ShellfoxSetup.exe` (`Shellfox.exe` in an `app-<version>` folder beside Squirrel's `Update.exe`). The helper waits for every process under the install root, then runs the new `ShellfoxSetup.exe --silent`.
- Linux x64 and ARM64 from the `shellfox` `.deb`, with `apt-get`, `dpkg-deb`, `sha256sum` and `pkexec` present. Before staging, the app checks the package name, version and architecture with `dpkg-deb`. The install runs `pkexec apt-get install`, which needs a graphical PolicyKit agent.
- macOS x64 and ARM64 `Shellfox.app` (bundle ID `local.shellfox`) in a writable folder. The ZIP is checked for unsafe entries, extracted with `ditto`, and must have the expected bundle ID and version and pass `codesign --verify --deep --strict`. The new app is staged beside the old one and swapped in with two renames. If the second rename fails, the old app is restored.

Anything else, including the Windows ZIP, the Linux `.tar.gz`, other architectures and source builds, shows the reason and a link to the releases page instead of a download button.

## `shellfox update` on a desktop install

When `shellfox-cli` sits next to a desktop binary, `shellfox update [--check]` runs `Shellfox --cli-update [--check]` in the same console (`crates/shellfox-desktop/src/update/cli.rs`). It uses the same release and package checks as the app, with no dialog. It rejects `--json` and remote `--host`.

`update` refuses while this installation's app, its desktop host or another `shellfox` command is running. Use the button in the app, or quit Shellfox and run it again. On Linux it installs with `sudo apt-get` when run in a terminal, otherwise `pkexec`. On macOS it swaps the bundle in the foreground. On Windows the helper finishes in the background, because Setup kills every process under the install root, the CLI included. Start Shellfox again a few seconds later.

Exit codes are 0 for up to date, available or installed, 1 for a failure or Shellfox still running, 2 for a usage error and 3 for an install it can't update.

## Headless packages

`shellfox update` in an extracted headless package downloads the archive for its OS and architecture, extracts and validates it next to the current installation, then points `headless-update.json` in the original installation at it. The running host is left alone until you restart it. `--json` works here. See [headless updates](headless.md#updates) for launcher paths, restart, rollback and locking.

## Release checks and pre-releases

Every updater reads `releases/latest` from the GitHub API and drops a release that is a draft, is marked pre-release, or has a pre-release or build suffix in its tag. The release workflow publishes any tag with a `-`, such as `v0.10.0-beta.1`, as a pre-release and doesn't mark it latest, so no updater offers it. Testers install those by hand. A test build such as 0.10.0-beta.2 is offered the final 0.10.0, which sorts above it.

The updater picks the asset by its exact name (`ShellfoxSetup.exe`, `shellfox_<version>_<amd64|arm64>.deb`, `Shellfox-darwin-<x64|arm64>-<version>.zip`, or the headless archive). It needs exactly one match, a download URL under the release's own `download/<tag>/` path, a size between 1 byte and 768 MiB and a SHA-256 digest in the metadata. Redirects may only go to GitHub's asset hosts. The file must match the size and digest. Shellfox checks the digest again before it hands the file to the installer. This trusts the GitHub repository and whoever publishes its releases. There is no separate signing key.

## Updating from Electron 0.9.15

0.10.0 keeps the 0.9 installer names and package IDs: Squirrel `shellfox`, deb `shellfox`, macOS `local.shellfox`. So 0.9.15's own updater installs it in place like any other release, and the Rust app handles the Squirrel install events. Linux 0.10.0 needs glibc 2.31. On older distributions the 0.9.15 updater reports a failed install and keeps the old app (see [platform support](platform-support.md#linux-glibc-floor)).

On its first start, a host whose data directory is `<Shellfox>/rust-v1/host` imports from the 0.9 files in `<Shellfox>` (`crates/shellfox-service/src/electron_import.rs`):

- settings and window bounds;
- saved sessions, without their shells;
- SSH profiles and known host keys;
- device approvals and remote access preferences from `remote.json`;
- the Tailscale node identity, copied, so paired devices keep working without a new login.

Each item fills only an empty Rust store, and marker files in the host's private directory make the import run once. The import reads SQLite from a private snapshot and doesn't change the 0.9 files. It holds Electron's `.host-lock` while the host runs, so 0.9 can't start next to it with the same node identity. Running terminals are not carried over.

Chromium caches are deleted right after the import. Everything else stays for 30 days, so you can reinstall 0.9.15 and find its data. When the 30 days are up, the host deletes the 0.9 files it found at import time (the database, `remote.json`, SSH files, `tailscale/`, Chromium profile data and the lock directory), by exact name and without following links. It skips the cleanup for good if any of those files changed after the import, because that means 0.9 ran again. Unknown files and `rust-v1` are never touched. If 0.9 ran again, the next start merges what it changed: sessions created or used in 0.9 since the last import are added or updated, unless the Rust copy changed later. Settings fields changed in 0.9 are copied too, and new SSH profiles and host keys are added. Sessions deleted in 0.9 stay in Rust, and device approvals are not merged. 0.9 never sees changes made in Rust, so going back to 0.9 shows its data as of the last time it ran.
