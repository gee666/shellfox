# Rust packaging

## Release installers

Run `node scripts/package-rust.mjs` on the target OS and architecture. It reads the version from `package.json`, builds `shellfox` and `shellfox-cli` in release mode, and writes to `tmp/rust-installers/`. Set `CARGO_TARGET_DIR` to give the build its own cache. Use the Ghostty, Go, and Windows SDK prerequisites below. Windows also needs `pnpm install --frozen-lockfile --ignore-scripts` for the existing Squirrel tooling.

- Windows x64: `ShellfoxSetup.exe`, the `shellfox` Squirrel package and `RELEASES`, and a portable ZIP. Windows ARM64 packages are ZIP-only.
- Linux x64 and ARM64: `shellfox_<version>_<arch>.deb` and a portable `.tar.gz`. The DEB retains `/usr/lib/shellfox/shellfox` so Electron can hand off to it.
- macOS x64 and ARM64: a signed `Shellfox.app` in a DMG and the ZIP read by the existing macOS updater. CI builds both.

`node scripts/package-rust-smoke.mjs` extracts a package and exercises a real PTY without installing over another copy. Do not run `ShellfoxSetup.exe` on a machine with an existing Shellfox installation for a smoke test. The package script accepts a separate output directory and a `ShellfoxLaneI…` test identity for an isolated Windows install. Its name and shortcuts differ from the production installer.

Installed desktop builds default to `Shellfox/rust-v1/host` beneath the per-user app-data directory. Production Windows launches the Tailscale helper from the same stable path Electron uses. For development, pass an isolated `--data-dir` and the pinned development helper. The installer does not publish or import data.

## Experimental portable archives

This is migration infrastructure, not a release channel. It does not replace the installed Shellfox, install startup jobs, enroll Tailscale, request UAC, sign binaries, or publish anything. The existing Electron and Node-headless packages remain separate products until runtime and installer parity is proven.

## Commands and prerequisites

Run from this worktree with Node 24. The CI pin is 24.22.1. Rust comes from `rust-toolchain.toml`, currently 1.97.1. Go must be exactly 1.27.3 for these packages. `SHELLFOX_GO` may select that executable. Dependency versions come from `Cargo.lock`, the Go helper's `go.mod` and `go.sum`, and Ghostty's retained signed archive manifest.

```sh
node --test scripts/rust-package.test.mjs
cargo install cargo-audit --locked --version 0.22.2 --root tmp/rust-package-tools --target-dir tmp/rust-package-audit-build
export PATH="$PWD/tmp/rust-package-tools/bin:$PATH"
node scripts/rust-package-security.mjs fetch-db
node scripts/rust-package-prereqs.mjs
node scripts/rust-package.mjs headless
node scripts/rust-package.mjs desktop
```

`rust-package-prereqs.mjs` explicitly acquires checksum-verified Ghostty archives. Windows x64 uses the retained official 1.0.1 DLL archive. Other native targets acquire Zig 0.17.0 with the SHA-256 in `crates/ghostty-vt-sys/build-support/zig-0.17.0.json`, then build the pinned Ghostty source and dependency archives. Outside Actions, set `GHOSTTY_VT_DIR` to the printed installation path. A pre-existing source-build prefix is refused rather than silently reused. These recipes are not evidence that a target builds or runs.

Windows desktop packaging also builds a static Linux WSL Files worker for the package architecture. `rust-package-prereqs.mjs` adds the pinned Rust musl target; packaging embeds the worker in `Shellfox.exe` and includes the toolchain notices. Local WSL Files needs WSL 2 and `/usr/bin/python3`. The host records the ordinary account when a new WSL tab first launches. The worker checks that account's effective UID even if the distro default changes. Older tabs without launch proof need a new tab. The worker runs from memory and does not install anything in the distro. A raw development build without `SHELLFOX_WSL_FILES_BINARY` can run Windows Files, but cannot open WSL Files.

Windows needs Visual Studio C++ tools, the Windows SDK, and the .NET SDK. The active helper uses project-local SDK 10.0.404 and self-contained runtime 10.0.15 through `scripts/dotnet-sdk.mjs`. Provision it explicitly with `node scripts/dotnet-sdk.mjs provision`; `SHELLFOX_DOTNET` may select an absolute, version-checked `dotnet.exe`. It does not replace a global SDK or change PATH. GPUI needs the SDK's `fxc.exe`. Use a Visual Studio developer shell or set `GPUI_FXC_PATH` to an installed compiler, for example `C:/Program Files (x86)/Windows Kits/10/bin/10.0.22000.0/x64/fxc.exe`. GPUI 0.2.2's fallback checks a different SDK version and can miss an installed compiler.

MSVC still rejects some long header paths. The packaging script uses the short, separate native build cache `tmp/rp-<platform>-<arch>/` and omits the redundant native target-triple directory. A first attempt with the longer cache produced a 261-character `grid_ref_tracked.h` path. The header existed and the archive was valid; MSVC could not open it. Do not change registry policy or stop a running app to solve that build-path constraint.

Linux native builds need a C/C++ compiler, clang, cmake, pkg-config, OpenSSL, fontconfig, FreeType, X11/XCB, xkbcommon, Wayland, Vulkan, ALSA and udev development packages. The workflow lists the Ubuntu 24.04 package names. These system packages come from the runner image and Ubuntu repositories, not a content-addressed sysroot. Capture the runner image and installed package versions for release qualification. A non-window desktop smoke does not prove a working GPU, X11 or Wayland session.

macOS requires Xcode command-line tools and the macOS SDK. Retained native C helpers use `xcrun --sdk macosx clang`, strict warnings, libproc and deployment target 13.0. This path does not call `codesign`, notarization, or Forge. An unsigned portable directory is not a distributable `.app` bundle.

## Output and isolation

The script discovers the workspace binary targets and fails if one is absent. It builds `shellfox-cli` for headless packages and both `shellfox-cli` and `shellfox` for desktop packages, with `cargo build --locked --release`. The artifact version comes from `package.json`.

Builds never overwrite `target/debug`, `target/release` or installed app directories. Disposable staging and extracted smoke fixtures stay under root `tmp/` and are removed after use. The short `tmp/rp-*` build cache, prepared Ghostty installation and successful artifacts are retained as deliberate build outputs. A host that cannot stop gracefully causes failure and retains its isolated scratch directory and PID for investigation. The script does not force-stop it or remove its live data.

Packaging now requires a passing strict license-text inventory and no active RustSec vulnerabilities before creating an archive. The retained earlier runtime-smoke archives below predate those gates and are not accepted deliverables. Failed audit reports remain under `tmp/rust-package-reports/` rather than being discarded with staging.

Successful archives live under `tmp/rust-package-artifacts/`:

```text
shellfox-rust-<desktop|headless>-<Cargo version>-<platform>-<arch>.zip|tar.gz
<archive>.sha256
<archive>.acceptance.json
```

An existing artifact is not overwritten. A failed archive smoke removes the incomplete archive and sidecars. Smoke tests extract the newly built archive to a path containing spaces, verify every file hash, launch only the extracted binaries, and use new isolated data directories. No installed app process is enumerated, contacted, closed or killed.

## Resource layout and source trace

```text
package-manifest.json
bin/
  shellfox-cli[.exe]
  shellfox[.exe]       desktop only
  ghostty-vt.dll                     Windows, beside loading binaries
resources/
  tailscale/
    shellfox-tailscale[.exe]
    THIRD-PARTY-NOTICES.txt
    TAILSCALE-LICENSE
    NOTICE-AUDIT.md
    modules.json
  elevated-terminal/                Windows desktop only, whole publish output
    Shellfox.ElevatedTerminal.exe
    Shellfox.ElevatedTerminal.dll
    Shellfox.ElevatedTerminal.deps.json
    Shellfox.ElevatedTerminal.runtimeconfig.json
    hostfxr.dll, hostpolicy.dll, coreclr.dll, remaining runtime assets
    DOTNET-LICENSE.TXT
    DOTNET-THIRD-PARTY-NOTICES.TXT
    notice-provenance.json
  terminal-native/                  macOS, desktop and headless
    shellfox-process-snapshot
    shellfox-terminal-supervisor
notices/
  cargo/<crate>-<version>/...
  ghostty-vt/...
dependency-licenses.json
cargo-tree.txt
THIRD-PARTY-NOTICES.txt
LICENSE
shellfox[.cmd|.ps1]                  CLI launcher
shellfox-desktop[.cmd|.ps1]          desktop launcher, desktop package only
```

The Go helper's producer is `scripts/tailscale.mjs`; its active consumer path is `src/main/index.ts`, `resources/tailscale/shellfox-tailscale[.exe]`. The Rust CLI takes an explicit helper path and trusted SHA-256. Packaging rebuilds the actual Go source, retains the generated module notices and probes it without enrollment. It does not substitute an arbitrary executable.

The active Windows helper is `native/Shellfox.ElevatedTerminal`, built by `scripts/terminal-native.mjs` in the existing app. `src/main/terminal/windows-elevated.ts` resolves `resources/elevated-terminal/Shellfox.ElevatedTerminal.exe`. Rust packaging performs the same target-native self-contained publish into its own stage and keeps the whole result. Its `probe` command is non-elevated. The normal Windows Rust PTY supervisor lives in the actual desktop/CLI executable and is dispatched before regular argument parsing; there is no separate retired input helper to bundle. Do not add `ShellFox.Windows.dll`, `walter-windows-input.exe`, Electron, node-pty or a .NET runtime shim without a current runtime dependency.

Darwin helper sources and filenames come from `scripts/terminal-native-helpers.mjs` and `src/main/terminal/native/`. Both helpers retain executable modes. Packaging them does not prove that the Rust Darwin adapter, lifecycle ownership or actual shell cleanup works. There is no packaged development-path fallback.

Linux does not receive unrelated C# helpers, Python supervisor scripts or Electron assets. Any new Rust adapter prerequisite must be added explicitly and validated on that OS.

## Managed runtime audit

The desktop finds the elevated helper at `resources/elevated-terminal/Shellfox.ElevatedTerminal.exe` next to its executable (`../resources` from `bin/`). Windows packaging writes `managed-inventory.json` and `managed-vulnerability-report.json` into that folder from the real publish `.deps.json` and `.runtimeconfig.json`. The NuGet advisory snapshot (`SHELLFOX_NUGET_VULNERABILITY_SNAPSHOT`, or fetched during packaging) is separate from the Cargo audit. Packaging stops if the report lists findings, is incomplete, or the .NET channel is unsupported, and copies the evidence to `tmp/rust-package-native-audit-failure-*`.

## CLI and user data

Launchers forward arguments unchanged. They do not set hidden environment aliases, change PATH, select a production profile, install a service, or require Node at runtime. Windows includes PowerShell and cmd launchers. Unix launchers use `/bin/sh` and `exec` so exit status and signals reach the Rust binary.

The current CLI contract is:

```text
shellfox --data-dir ABSOLUTE_NEW_DIRECTORY --helper ABSOLUTE_PACKAGED_HELPER --helper-sha256 TRUSTED_MANIFEST_DIGEST host serve
shellfox --data-dir ABSOLUTE_NEW_DIRECTORY --helper ABSOLUTE_PACKAGED_HELPER --helper-sha256 TRUSTED_MANIFEST_DIGEST session list
shellfox --data-dir ABSOLUTE_NEW_DIRECTORY --helper ABSOLUTE_PACKAGED_HELPER --helper-sha256 TRUSTED_MANIFEST_DIGEST host stop
```

Create the new experiment data directory first. Read the helper digest from the trusted build's `package-manifest.json` entry for `resources/tailscale/shellfox-tailscale[.exe]`. A SHA-256 calculated from an arbitrary supplied executable is not proof that it is a trusted helper. Archive sidecars detect corruption; they are not signatures or authenticated release metadata.

The desktop takes `--data-dir ABSOLUTE_NEW_DIRECTORY`. `--smoke` runs the owned PTY test before GPUI initialization and does not open a user window. Do not assume `--version` exists. No `SHELLFOX_USER_DATA_DIR` or `SHELLFOX_RESOURCES_DIR` support is promised by this package. A future explicit resource adapter must validate its own contract.

## Dependency boundaries and notices

Headless packaging records `cargo tree --locked --target <native triple> --edges normal,build --prefix none --format '{p}'`. It rejects GPUI, desktop, GPU and Electron crate names in that graph and checks the actual package inventory for browser/native Node runtime assets. The Ghostty VT library is intentional and does not include Ghostty's desktop renderer. These checks run on each target because Cargo target-specific dependencies differ.

`rust-package-audit.mjs` joins that selected graph to actual `cargo metadata --locked` records and the exact name/version/source/checksum entries in `Cargo.lock`. It deliberately uses unfiltered metadata. A cross-target tree includes build-host dependencies that `--filter-platform` can omit. The selected tree still constrains the inventory to the target and actual build host. It keeps each package's SPDX expression, repository, license text files and file digests. It does not relabel dependencies as MIT. Workspace MIT crates may use the root license; third-party path dependencies must provide their own text. Build dependencies are included conservatively, not claimed to be linked into every binary.

```sh
node scripts/rust-package-audit.mjs x86_64-pc-windows-msvc shellfox-cli tmp/rust-package-audit --strict
```

The JSON inventory marks missing expressions/text as `reviewRequired` and always records `legalApproval: false`. `--strict` fails when text is unresolved. Packaging always enables that gate. It is audit tooling, not a legal sign-off.

Supplemental inputs are in `scripts/rust-package-license-inputs.json` and `scripts/rust-package-license-texts/`. Their records preserve the locked crate checksum, exact upstream commit, URL, Git blob hash and text SHA-256. Where `.cargo_vcs_info.json` exists, missing files were sought at that recorded commit after checking the cached `.crate` against Cargo.lock. Older crates without that record remain unresolved unless their release source can be matched; a repository HEAD is not treated as release provenance. The old `mac` release tag was checked by byte-comparing its manifest, README and every source file with the locked crate. Upstream declarations and README statements are retained as declarations, not counted as complete license texts. MIT/Apache alternatives remain intact when both texts exist. `svg_fmt` selects its actual MIT text; `zune-inflate` selects its actual Zlib text while retaining the upstream MIT/Apache declaration. No copyright line was invented.

For the original Windows graph, 22 of 27 missing texts were recovered. These five still need an authoritative upstream grant/text, an approved dependency change, or a user decision: `convert_case 0.4.0`, `hexf-parse 0.2.1`, `mac 0.1.1`, `seahash 4.1.0`, `simd_helpers 0.1.0`. They remain blocked. Cross-target scans found additional Apple and Linux gaps. Recovered exact texts for `pathfinder_simd`, Wayland Plasma and XIM are retained too. `pathfinder_geometry 0.5.1` records an upstream commit that is unavailable at the repository. Older `block`, `leak`, `leaky-cow` and `malloc_buf` releases lack full texts. The `objc2` family's `LICENSE.md` is a declaration with external links and an explicit Apple SDK derived-work licensing caveat, not a complete license text. Those require review, not automatic approval.

### Security policy

`rust-package-security.mjs` uses cargo-audit 0.22.2 and a project-local RustSec database pinned at commit `4ef2df8e806a65e2f1ecc14f56f51249062b23d6`. Acquisition is explicit through `fetch-db`; audits use `--no-fetch`, reject a database commit older than 90 days and do not edit global advisory configuration. Missing/error JSON reports, advisory-ignore lists and severity filters fail closed. Review and update the pin as part of future qualification. A fixed snapshot cannot cover later advisories.

```sh
node scripts/rust-package-security.mjs audit headless tmp/rust-package-security-headless
node scripts/rust-package-security.mjs audit desktop tmp/rust-package-security-linux x86_64-unknown-linux-gnu
```

The raw full-lock report and a selected-tree report are retained. Policy denies active normal/build vulnerabilities. Inactive optional lockfile findings are still listed and are not an advisory waiver; maintenance warnings are reported for review. Reports record the actual compiler host and audit the union of target/build-host OS and architecture constraints because cross-target analysis on Windows is not a native Linux/macOS build. The offline check does not check yanked status, the OS sysroot, Go vulnerabilities or runtime reachability within a linked crate. It does not replace a release security review.

The first real scan caught active `RUSTSEC-2026-0459` in `libsqlite3-sys 0.30.1`. The host owner upgraded to `rusqlite 0.41.0` with bundled `libsqlite3-sys 0.39.1`, SQLite 3.54.1. Follow-up scans covered all six portable targets for both package families and found no active RustSec vulnerabilities. RSA `RUSTSEC-2023-0071` and crypto-bigint `RUSTSEC-2026-0387` remain inactive full-lock findings, not ignored exceptions. Desktop maintenance warnings remain visible. Go retains the existing per-target compiled-module inventory and `missingModuleRootLicenses` report. Ghostty retains its own notices and provenance, including supplemental uucode texts. Windows runtime notices are fetched from the exact .NET 10.0.15 tag and checked against pinned SHA-256 values before inclusion. Review all three inventories and any missing-text reports before distribution.

## Update adapter requirements

The package manifest format is `shellfox-rust-portable-v1`. It records the actual binaries, resource root, per-file SHA-256 values, target, Cargo lock digest, and `update.supported: false`.

The existing updater expects Windows Squirrel installers, Linux DEBs, macOS signed app-bundle ZIPs, or the Node-headless runtime/update-pointer layout. None accepts this Rust directory format. Do not reuse those asset names, forge an Electron `runtime.json`, or report automatic updates as supported.

A later Rust adapter needs explicit format/version selection, executable-relative resource resolution, target and archive validation, authenticated digest provenance, traversal/symlink rejection, unchanged permission checks, readiness/commit/cancel handoff, owned-terminal shutdown, rollback and retained data-schema rules. It must refuse unsupported install formats before touching terminals. `resources/` being present is not an implementation of that adapter.

## Platform matrix and acceptance

`.github/workflows/rust-platforms.yml` is a distinct manual workflow. Its default runs packaging rules only. `build_native_packages=true` opts into native builds, archive smokes and evidence uploads for all six rows. No workflow was triggered for this work. Actions are commit-pinned; Rust, Go, Node, .NET and Zig versions are explicit. Runner images and apt repositories can still change. The workflow has read-only contents permission and no release, signing, startup-registration or installer step.

| Target | Build prerequisites | Acceptance status |
| --- | --- | --- |
| Windows x64 MSVC | Official pinned Ghostty DLL, VS C++/SDK/fxc, Go, self-contained elevated helper | Original release-profile desktop/headless runtime archive smokes passed. Desktop license gaps block accepted rebuilds. GUI/UAC/full platform acceptance not claimed |
| Windows arm64 MSVC | Native ARM runner/toolchain, source-built target-matching Ghostty DLL/import library, ARM64 Go and .NET helper | Configured, not built or runtime-verified here |
| Linux x64 GNU | Pinned source Ghostty, native compiler and GPUI system development libraries | Configured, not built or runtime-verified here |
| Linux arm64 GNU | Same, native ARM compiler/runtime | Configured, not built or runtime-verified here |
| macOS x64 | Source Ghostty, Xcode/macOS SDK, both native C helpers | Configured, not built or runtime-verified here; portable output is not a signed app bundle |
| macOS arm64 | Same, native ARM runner and libraries | Configured, not built or runtime-verified here |
| Android | Remote-only mobile Rust binding and native Ghostty component; JDK 17, Android SDK/build tools 36.0.0, NDK 27.1.12297006, Gradle 9.4.1, Cargo target-specific linker/sysroot; API 24 minimum | Separate mobile work in progress, not a desktop/headless portable package or verified APK |
| iOS | Remote-only mobile Rust binding and native Ghostty component; macOS/Xcode, exact SDK, device/simulator Cargo targets, iOS 15.1 deployment target, React Native/CocoaPods integration | Planned native build/launch gate, not a verified IPA or App Store build |

For mobile, use target-specific static Ghostty builds. Android minimum API must remain 24, not the old generic API 26 recipe. iOS must remain 15.1, not generic 13.0. Do not reuse desktop flags, GPUI, host/PTY features, Windows DLLs or Go desktop helper directories in mobile packages. Mobile owners must supply the linked ABI outputs and update this matrix with actual device/simulator evidence. Signing credentials and distribution remain outside this workflow.

A successful `acceptance.json` covers the extracted file hashes, offline Go probe, CLI launcher, new isolated persistent host, metadata CRUD/settings and graceful `host stop`. Desktop adds its owned PTY/Ghostty input/output/resize/cleanup smoke and Windows adds the non-elevated helper probe. That still does not prove GUI parity, UAC behavior, startup/reboot, remote connectivity, updates, signed distribution, Android or iOS. Run those gates on their real systems and retain failures rather than marking configured targets as verified.

## Local evidence

- `node --test scripts/rust-package.test.mjs`: 12 passed. The workflow YAML also parsed with six native matrix entries.
- `pnpm typecheck`: passed.
- `pnpm test:unit`: 338 files passed, 4039 tests passed, 59 skipped. Log retained at `tmp/rust-package-unit.log`.
- Actual Windows x64 release desktop and headless archives passed extracted-archive E2E. Both exercised the new private host endpoint, metadata CRUD/settings and graceful stop. Desktop also passed its owned PTY/Ghostty input/output/resize/cleanup smoke without a GUI window, plus the non-elevated self-contained helper probe.
- Locked headless tree/license inventory covered 111 packages, with no missing Cargo license expressions or text files. Desktop covered 494 packages; 27 crate archives omit license texts and remain `reviewRequired`. Examples include blade, lyon, naga, taffy and the CC0-1.0 hexf-parse crate. Expressions remain intact. This is a distribution blocker, not permission to substitute generic MIT text. The Go compiled-module inventory reported no missing module-root license files.
- The first extracted-archive host smoke found the canonical Windows path mismatch. Service owners fixed the narrow ordinary-drive conversion; the final E2E above passed without weakening Go validation. Earlier failed archives were removed. Desktop compilation used explicit `GPUI_FXC_PATH` for the installed SDK.

These original artifacts prove runtime-smoke behavior only. They contain the earlier SQLite dependency and pre-supplement notices, so neither is legal/security packaging acceptance. They remain unchanged for evidence. Rebuilding accepted outputs is blocked on the license decisions above.

Retained experimental archives and SHA-256 values:

| Artifact under `tmp/rust-package-artifacts/` | SHA-256 |
| --- | --- |
| `shellfox-rust-headless-0.1.0-win32-x64.zip` | `9b28fdb99ce580d7b3b75e23a1a83391eecabb82459b0a97f673e68697dbf57d` |
| `shellfox-rust-desktop-0.1.0-win32-x64.zip` | `d0414027c689772fb44249e0fa0b0fa89eb5387c59b9fcf9ac9956948d35b840` |

Latest static evidence is retained in `tmp/rust-package-license-targets.json` and `tmp/rust-package-security-all/`. Against Cargo.lock SHA-256 `1ce6d2d74bd6e7a771501c68cc9a13bb1107d28ab34ba07ebe6b56eb9fae96d3`, all six headless trees pass the no-GPU and license-text checks. Desktop strict text gaps remain 5 on Windows, 5 on Linux and 17 on macOS. The supplemental inventory records 44 versioned investigations, 26 with recovered complete texts and 18 distinct unresolved across targets. Twelve security scans found zero active vulnerabilities; desktop has 7 active maintenance warnings on Windows/macOS and 13 on Linux, with none on headless. These are Windows-host static target analyses, not non-Windows executable tests.

Each original archive has `.sha256` and `.acceptance.json` sidecars. These are local artifacts from the tested source state, not published releases or proof of later concurrent source changes.

No release workflow, signing operation, service installation, account login or installed-app replacement was performed.
