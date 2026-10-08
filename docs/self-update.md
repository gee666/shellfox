# In-app updates

Installed builds check the latest stable GitHub release at startup and once an hour while running. The renderer refreshes the notice once a minute and polls download progress twice a second. The compact sidebar control shows only a download button, a progress bar with estimated time remaining, or an install button.

Click **download vX.Y.Z** to fetch and verify an installer without interrupting any terminal. Once the download is ready, click **install and restart**. A native confirmation defaults to Cancel and explicitly warns that all embedded terminals and their running commands will stop. Cancellation leaves the download ready and terminals running. Legacy external terminals are never closed.

After confirmation, the main process starts an installer helper and waits for a readiness acknowledgement before closing any terminal. Terminal closure is reversible at the service level. It pauses new launches and closes owned shells without disposing the backend or subscriptions. Once exit metadata has been saved, the main process sends commit and waits for a second helper acknowledgement. Only then does it finalize the service and quit. The helper requires both explicit commit and manager-process exit before replacing files. Restarting creates fresh shells; running commands do not survive an update.

## Supported installations

- Windows x64 installed through `ShellfoxSetup.exe`, with Squirrel's `Update.exe` present. Windows ZIP and ARM64 builds require manual updates.
- Linux x64 and ARM64 `.deb` installations whose executable belongs to the `shellfox` package. In-app installation requires `apt-get`, `sha256sum`, `pkexec` and a working graphical PolicyKit authentication agent. The installer checks the package name, version and architecture before offering installation.
- macOS x64 and ARM64 `Shellfox.app` bundles in a writable parent directory. ZIP contents, bundle identifier, version and code-signature validity are checked before staging. Replacement uses same-filesystem renames and attempts rollback if the second rename fails. Read-only volumes and non-writable Applications directories require manual installation.

## Trust and failure handling

The renderer cannot supply a URL, filesystem path or command. IPC validates the exact main-frame sender and strict payload schemas. Installation additionally requires native confirmation in the main process.

The updater only selects the expected asset name from the pinned `gee666/shellfox` GitHub repository. It rejects draft/prerelease versions, duplicate matching assets, unexpected download locations, missing SHA-256 digests, empty files and files larger than 768 MiB. The download must match both the API's size and SHA-256 digest. A missing GitHub asset digest disables in-app installation for that release. This trusts the GitHub repository and its release publisher; it does not add a separate publisher-signing key.

macOS ZIP validation rejects traversal, absolute paths, duplicate/case-colliding paths, mismatched local headers, unsupported archive formats and symlinks that escape the bundle or have archive entries written beneath them. macOS ZIP64 archives are unsupported. Existing ad-hoc code signatures are accepted when `codesign --verify --deep --strict` succeeds; this is an integrity check, not an independent publisher identity check. Quarantine attributes are not stripped.

Offline checks retain the last known release. Failed downloads show an error and can be retried. Normal quit cancels any uncommitted helper, aborts pending downloads and removes staged files. Confirmed installation hands the temporary directory to the detached helper. Successful installation removes it; failed installations retain `install.log` inside the OS temporary directory's `shellfox-update-*` folder. Windows and macOS show a failure dialog. Linux does so when `zenity` is installed. The helper attempts to reopen the previous app after an installer failure or declined elevation.

Helper startup failures leave terminals untouched. Terminal-close or commit failures cancel the helper before resuming the service, and installation can be retried or the app can be quit normally. If cancellation cannot be confirmed, the service stays paused until a retry or normal quit confirms cancellation. Previously closed shells cannot be restored, but the retained backend can open fresh shells after recovery. Exit-persistence failures also leave the backend intact rather than exposing a manager backed by a disposed terminal service.

## Tests

Unit tests cover release selection, size/digest verification, streaming progress, concurrent requests, hourly checks, abort/cleanup, explicit IPC consent, compact UI progress and ETA, helper readiness/commit/cancellation with real child processes, reversible shutdown and fresh-shell recovery, platform installer arguments, Linux package identity, macOS staging/rollback commands and hostile ZIP entries. The Windows helper is parsed by Windows PowerShell without executing an installer. The macOS packaged smoke test runs the same strict code-signature verification as the updater, matching Forge's default signing verification for both ad-hoc and certificate-signed bundles. These tests do not install real packages or execute downloaded installers. Platform release-install smoke tests are still required before distributing an updater release.
