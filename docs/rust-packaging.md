# Rust packaging

## Release installers

Run `node scripts/package-rust.mjs` on the target OS and architecture. It reads the version from `package.json`, builds `shellfox` and `shellfox-cli` in release mode, and writes to `tmp/rust-installers/`. Set `CARGO_TARGET_DIR` to give the build its own cache. Use the Ghostty, Go, and Windows SDK prerequisites below. Windows also needs `pnpm install --frozen-lockfile --ignore-scripts` for the existing Squirrel tooling.

- Windows x64: `ShellfoxSetup.exe`, the `shellfox` Squirrel package and `RELEASES`, and a portable ZIP. Windows ARM64 packages are ZIP-only.
- Linux x64 and ARM64: `shellfox_<version>_<arch>.deb` and a portable `.tar.gz`. The DEB retains `/usr/lib/shellfox/shellfox` so Electron can hand off to it.
- macOS x64 and ARM64: a signed `Shellfox.app` in a DMG and the ZIP read by the existing macOS updater. CI builds both.

`node scripts/package-rust-smoke.mjs` extracts a package and exercises a real PTY without installing over another copy. Do not run `ShellfoxSetup.exe` on a machine with an existing Shellfox installation for a smoke test. The package script accepts a separate output directory and a `ShellfoxTest…` test identity for an isolated Windows install. Its name and shortcuts differ from the production installer.

Installed desktop builds default to `Shellfox/rust-v1/host` beneath the per-user app-data directory. Production Windows launches the Tailscale helper from the same stable path Electron uses. For development, pass an isolated `--data-dir` and the pinned development helper. The host imports 0.9 data on its first start, not the installer.

## Portable archives

`scripts/rust-package.mjs` builds portable desktop and headless directories for the `rust-platforms.yml` CI workflow. They are test builds, not installers: they don't sign, install startup jobs or update themselves. Each archive gets a `.sha256` and an `.acceptance.json` smoke record.

## Commands and prerequisites

Use Node 24.22.1. Rust comes from `rust-toolchain.toml`, currently 1.97.1. Go must be exactly 1.27.3 for these packages. `SHELLFOX_GO` may select that executable. Dependency versions come from `Cargo.lock`, the Go helper's `go.mod` and `go.sum`, and Ghostty's retained signed archive manifest.

```sh
node --test scripts/rust-package.test.mjs
cargo install cargo-audit --locked --version 0.22.2 --root tmp/rust-package-tools --target-dir tmp/rust-package-audit-build
export PATH="$PWD/tmp/rust-package-tools/bin:$PATH"
node scripts/rust-package-security.mjs fetch-db
node scripts/rust-package-prereqs.mjs
node scripts/rust-package.mjs headless
node scripts/rust-package.mjs desktop
```

`rust-package-prereqs.mjs` explicitly acquires checksum-verified Ghostty archives. Windows x64 uses the retained official 1.0.1 DLL archive. Other native targets acquire Zig 0.17.0 with the SHA-256 in `crates/ghostty-vt-sys/build-support/zig-0.17.0.json`, then build the pinned Ghostty source and dependency archives. Outside Actions, set `GHOSTTY_VT_DIR` to the printed installation path. A pre-existing source-build prefix is refused rather than silently reused.

Windows desktop packaging also builds a static Linux WSL Files worker for the package architecture. `rust-package-prereqs.mjs` adds the pinned Rust musl target; packaging embeds the worker in `Shellfox.exe` and includes the toolchain notices. Local WSL Files needs WSL 2 and `/usr/bin/python3`. The host records the ordinary account when a new WSL tab first launches. The worker checks that account's effective UID even if the distro default changes. Older tabs without launch proof need a new tab. The worker runs from memory and does not install anything in the distro. A raw development build without `SHELLFOX_WSL_FILES_BINARY` can run Windows Files, but cannot open WSL Files.

Windows needs Visual Studio C++ tools, the Windows SDK, and the .NET SDK. The active helper uses project-local SDK 10.0.404 and self-contained runtime 10.0.15 through `scripts/dotnet-sdk.mjs`. Provision it explicitly with `node scripts/dotnet-sdk.mjs provision`; `SHELLFOX_DOTNET` may select an absolute, version-checked `dotnet.exe`. It does not replace a global SDK or change PATH. GPUI needs the SDK's `fxc.exe`. Use a Visual Studio developer shell or set `GPUI_FXC_PATH` to an installed compiler, for example `C:/Program Files (x86)/Windows Kits/10/bin/10.0.22000.0/x64/fxc.exe`. GPUI 0.2.2's fallback checks a different SDK version and can miss an installed compiler.

MSVC still rejects some long header paths. The packaging script uses the short, separate native build cache `tmp/rp-<platform>-<arch>/` and omits the redundant native target-triple directory. A first attempt with the longer cache produced a 261-character `grid_ref_tracked.h` path. The header existed and the archive was valid; MSVC could not open it. Do not change registry policy or stop a running app to solve that build-path constraint.

Linux native builds need a C/C++ compiler, clang, cmake, pkg-config, OpenSSL, fontconfig, FreeType, X11/XCB, xkbcommon, Wayland, Vulkan, ALSA and udev development packages. The workflow lists the Ubuntu 24.04 package names. These system packages come from the runner image and Ubuntu repositories, not a content-addressed sysroot. A smoke without a window doesn't prove a working GPU, X11 or Wayland session.

macOS requires Xcode command-line tools and the macOS SDK. Retained native C helpers use `xcrun --sdk macosx clang`, strict warnings, libproc and deployment target 13.0. This path does not call `codesign`, notarization, or Forge. An unsigned portable directory is not a distributable `.app` bundle.

## Output and isolation

The script discovers the workspace binary targets and fails if one is absent. It builds `shellfox-cli` for headless packages and both `shellfox-cli` and `shellfox` for desktop packages, with `cargo build --locked --release`. The artifact version comes from `package.json`.

Builds never overwrite `target/debug`, `target/release` or installed app directories. Disposable staging and extracted smoke fixtures stay under root `tmp/` and are removed after use. The short `tmp/rp-*` build cache, prepared Ghostty installation and successful artifacts are retained as deliberate build outputs. A host that cannot stop gracefully causes failure and retains its isolated scratch directory and PID for investigation. The script does not force-stop it or remove its live data.

Packaging requires a passing strict license-text inventory and no active RustSec vulnerabilities before creating an archive. Failed audit reports remain under `tmp/rust-package-reports/` rather than being discarded with staging.

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

`scripts/tailscale.mjs` builds the Go helper into `resources/tailscale/shellfox-tailscale[.exe]`. Installed builds compile in its SHA-256; portable builds take `--helper` and `--helper-sha256`. Packaging rebuilds the actual Go source, retains the generated module notices and probes it without enrollment. It does not substitute an arbitrary executable.

The Windows administrator-terminal helper is `native/Shellfox.ElevatedTerminal`, published self-contained into `resources/elevated-terminal/`. Its `probe` command is non-elevated. The normal Windows PTY supervisor lives in the desktop and CLI executables themselves, so there is no other input helper to bundle.

Darwin helper sources and filenames come from `scripts/terminal-native-helpers.mjs` and `src/main/terminal/native/`. Both keep their executable modes. There is no fallback to development paths.

Linux gets no C# helpers, Python supervisor scripts or Electron assets.

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

Create the data directory first. Read the helper digest from the trusted build's `package-manifest.json` entry for `resources/tailscale/shellfox-tailscale[.exe]`. A SHA-256 calculated from an arbitrary supplied executable is not proof that it is a trusted helper. Archive sidecars detect corruption; they are not signatures or authenticated release metadata.

The desktop takes `--data-dir ABSOLUTE_NEW_DIRECTORY`. `--smoke` runs the owned PTY test before GPUI initialization and does not open a user window.

## Dependency boundaries and notices

Headless packaging records `cargo tree --locked --target <native triple> --edges normal,build --prefix none --format '{p}'`. It rejects GPUI, desktop, GPU and Electron crate names in that graph and checks the actual package inventory for browser/native Node runtime assets. The Ghostty VT library is intentional and does not include Ghostty's desktop renderer. These checks run on each target because Cargo target-specific dependencies differ.

`rust-package-audit.mjs` joins that selected graph to actual `cargo metadata --locked` records and the exact name/version/source/checksum entries in `Cargo.lock`. It deliberately uses unfiltered metadata. A cross-target tree includes build-host dependencies that `--filter-platform` can omit. The selected tree still constrains the inventory to the target and actual build host. It keeps each package's SPDX expression, repository, license text files and file digests. It does not relabel dependencies as MIT. Workspace MIT crates may use the root license; third-party path dependencies must provide their own text. Build dependencies are included conservatively, not claimed to be linked into every binary.

```sh
node scripts/rust-package-audit.mjs x86_64-pc-windows-msvc shellfox-cli tmp/rust-package-audit --strict
```

The JSON inventory marks missing expressions/text as `reviewRequired` and always records `legalApproval: false`. `--strict` fails when text is unresolved. Packaging always enables that gate. It is audit tooling, not a legal sign-off.

Supplemental inputs are in `scripts/rust-package-license-inputs.json` and `scripts/rust-package-license-texts/`. Their records preserve the locked crate checksum, exact upstream commit, URL, Git blob hash and text SHA-256. Where `.cargo_vcs_info.json` exists, missing files were sought at that recorded commit after checking the cached `.crate` against Cargo.lock. Older crates without that record remain unresolved unless their release source can be matched; a repository HEAD is not treated as release provenance. The old `mac` release tag was checked by byte-comparing its manifest, README and every source file with the locked crate. Upstream declarations and README statements are retained as declarations, not counted as complete license texts. MIT/Apache alternatives remain intact when both texts exist. `svg_fmt` selects its actual MIT text; `zune-inflate` selects its actual Zlib text while retaining the upstream MIT/Apache declaration. No copyright line was invented.


### Security policy

`rust-package-security.mjs` uses cargo-audit 0.22.2 and a project-local RustSec database pinned at commit `4ef2df8e806a65e2f1ecc14f56f51249062b23d6`. Acquisition is explicit through `fetch-db`; audits use `--no-fetch`, reject a database commit older than 90 days and do not edit global advisory configuration. Missing/error JSON reports, advisory-ignore lists and severity filters fail closed. Update the pin before a release; a fixed snapshot can't cover later advisories.

```sh
node scripts/rust-package-security.mjs audit headless tmp/rust-package-security-headless
node scripts/rust-package-security.mjs audit desktop tmp/rust-package-security-linux x86_64-unknown-linux-gnu
```

The raw full-lock report and a selected-tree report are retained. Policy denies active normal/build vulnerabilities. Inactive optional lockfile findings are still listed and are not an advisory waiver; maintenance warnings are reported for review. Reports record the actual compiler host and audit the union of target/build-host OS and architecture constraints because cross-target analysis on Windows is not a native Linux/macOS build. The offline check does not check yanked status, the OS sysroot, Go vulnerabilities or runtime reachability within a linked crate.

RSA `RUSTSEC-2023-0071` and crypto-bigint `RUSTSEC-2026-0387` remain inactive full-lock findings, not ignored exceptions. Desktop maintenance warnings remain visible. Go retains the existing per-target compiled-module inventory and `missingModuleRootLicenses` report. Ghostty retains its own notices and provenance, including supplemental uucode texts. Windows runtime notices are fetched from the exact .NET 10.0.15 tag and checked against pinned SHA-256 values before inclusion. Review all three inventories and any missing-text reports before distribution.

## CI

`.github/workflows/rust-platforms.yml` runs the packaging rules by default. `build_native_packages=true` also builds and smokes the portable archives on all six desktop targets. Release installers come from `release.yml` on a tag push.
