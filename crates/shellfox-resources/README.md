# Native resource trust

This crate is host-only. Android and iOS must not depend on it. It does not use GPUI, change DLL search policy, modify the registry, enroll accounts, request elevation or activate power settings.

## Trust anchor and API

An EXE hash does not authenticate a .NET application. The unchanged apphost can load a changed managed DLL, dependency configuration or runtime binary. A verified bundle therefore covers every file in the self-contained publish directory.

The caller supplies `ExpectedManifestDigest` from a trusted build record or a compiled package anchor. There is deliberately no constructor that reads an arbitrary manifest and treats its hash as trusted. SHA-256 alone proves neither publisher identity nor source authorization. An attacker who supplies the expected digest can supply the entire bundle.

```rust,no_run
use shellfox_resources::{
    compiled_manifest_anchor, BundleIdentity, Error, ProtocolId, RuntimeId,
    TrustedBundleId, VerifiedBundle,
};

fn safe_probe(root: &std::path::Path) -> shellfox_resources::Result<()> {
    let identity = BundleIdentity {
        id: TrustedBundleId::WindowsManagedHelper,
        rid: RuntimeId::WinX64,
        version: "0.1.0".into(),
    };
    let anchor = compiled_manifest_anchor(identity.id)?
        .ok_or(Error::Manifest("trusted build anchor not configured"))?;
    let bundle = VerifiedBundle::open(root, anchor, &identity)?;
    let executable = bundle.executable(
        "Shellfox.ElevatedTerminal.exe", ProtocolId::PowerGuardian,
    )?;
    let mut command = executable.command()?;
    command.command_mut().arg("probe-power");
    let mut child = command.spawn()?;
    let status = child.wait()?;
    if !status.success() {
        return Err(Error::Manifest("safe probe failed"));
    }
    Ok(())
}
```

Default managed `command()` accepts only the exact safe-probe argument for the selected protocol. Raw managed `path_for_spawn()` refuses with `LoaderPolicyRequired`. Elevation/power operations require `NoDynamicCodeLoad::from_trusted_review(expected_manifest_digest)` and `command_with_loader_policy(&policy)` or `path_for_spawn_with_loader_policy(&policy)`. This is an explicit trusted source/loader review bound to one manifest, not something to infer from matching hashes or probes. There is no deserializable manifest flag that grants it. Until that review or a proven immutable staging policy exists, privileged factories must remain unavailable.

Factories must validate the live helper response against `VerifiedExecutable::protocol()` before advertising a capability. A manifest claim is not a live UAC, cleanup or power rollback test. `FileSet` exposes the exact expected files for inventory, not a mutable exemption list.

The stable types are `NoDynamicCodeLoad`, `BundleIdentity`, `TrustedBundleId`, `RuntimeId`, `ExpectedManifestDigest`, `FileSet`, `ProtocolId`, `ProtocolContract`, `VerifiedBundle` and `VerifiedExecutable`. `TrustedManifestDigest` is an alias for the expected-digest type. The caller provides an exact bundle version and RID, not a platform guess. Entry protocol IDs are `ElevatedConpty`, `ElevatedIsolated`, `PowerGuardian`, `Tailscale`, `DarwinSnapshot`, `DarwinSupervisor` and `WslGuardian`. Version 1 capability sets are closed and checked against the resource kind.

## Manifest and file checks

`bundle-manifest.json` has format `shellfox-native-bundle-v1`. It records identity, runtime, entrypoint protocols and every other file's relative path, byte length and SHA-256. The manifest itself is authenticated by the separately supplied expected digest and held open too. Unknown fields, duplicate claims, unsupported versions and case aliases fail closed.

Bounds are 2 MiB for the manifest, 2048 files, 256 MiB per file, 2 GiB total payload, 4096 namespace entries and 16 path components. Paths use a strict portable ASCII alphabet. Absolute paths, traversal, backslashes, alternate data streams, reserved DOS names and trailing-dot aliases are rejected on every OS. No payload file or empty directory may appear outside the manifest-derived tree. Symbolic links, Windows reparse points, hardlinks and repeated file identities are rejected. Root ancestors are opened and inspected too. Windows execution paths come from original handles as volume-GUID paths, with physical ancestor handles also retained; a late drive-letter remapping is not used to select the executable. Mapped remote drives are refused.

Managed metadata supports the audited `net8.0`, `net9.0` and `net10.0` layouts. The framework moniker is derived from the exact declared canonical runtime version and must match the Windows target framework, runtime configuration and dependency target. This is metadata compatibility, not lifecycle or vulnerability clearance. The build's active-support/security gate still rejects EOL runtimes.

Managed bundles require the apphost, managed entry DLL, `.deps.json`, `.runtimeconfig.json`, hostfxr, hostpolicy, coreclr, clrjit and System.Private.CoreLib. The dependency file's RID, runtime pack version, asset list and the runtime configuration's self-contained framework version must match the manifest. Development `.runtimeconfig.dev.json` probing files and framework-dependent fallback configuration are rejected. There is no installed-runtime fallback in this contract.

Useful resource kinds are a Windows self-contained managed helper, the Go Tailscale helper, the two Darwin C helpers, and the four WSL Python modules. WSL scripts can be read with `read_verified(relative, maximum)` as authenticated owned bytes from original handles. That does not authenticate a guest interpreter, guest filesystem, copied file or process identity.

## Original-handle lifetime

On Windows, original ancestor/directory/file handles use `FILE_SHARE_READ` without write/delete sharing and `FILE_FLAG_OPEN_REPARSE_POINT`. The file that was hashed remains open. No hash-then-close-then-reopen shortcut is used. Managed DLLs, runtime binaries and configuration files therefore remain protected against ordinary write/delete/replacement opens while any `Arc<VerifiedBundle>` remains alive.

`VerifiedExecutable` owns that Arc. `VerifiedCommand` revalidates immediately before spawning and gives the helper its verified executable directory as working directory. Replacing the command program or working directory is rejected. Managed launch removes inherited or explicitly supplied CLR/DOTNET startup-hook and profiler variables from that child only. It does not alter the parent environment or a global DLL search path.

`VerifiedChild::wait` and `try_wait` release the process's bundle hold only after the OS confirms exit. Dropping an unconfirmed child retains its process handle and bundle in an in-process quarantine. Call `reap_unconfirmed_children()` to release confirmed exits. Errors retain ownership. No background thread creation, timeout or implicit kill can release an unconfirmed hold. Async adapters using the path APIs must keep their own `VerifiedExecutable` field until exit is confirmed, including failed cleanup, and must enforce equivalent environment/working-directory/protocol handling. Managed paths require the explicit loader review above. A wait confirms only that specific spawned child, not a detached/elevated peer. Elevation factories must keep an additional `VerifiedExecutable` clone until the actual broker and every other bundle-using peer are exit-confirmed. Launcher exit and transport EOF are not sufficient. The crate cannot retain handles after the owning process itself dies; process ownership and crash cleanup remain the host adapter's job.

### Go transport retention

The transport descriptor's `VerifiedHelper::with_resource_guard(Arc<dyn Send + Sync>)` hook retains an already validated artifact lease. In the composition root, use the executable path and image digest from the same externally anchored Go artifact, then attach its bundle Arc:

```text
let guard: Arc<dyn Send + Sync> = validated_go.bundle().clone();
let helper = VerifiedHelper::new(verified_path, verified_image_digest)?
    .with_resource_guard(guard);
```

If both bundle and cache leases are needed, retain both in one composite guard instead of replacing an existing lease. Pass that descriptor onward intact. Reconstructing a descriptor from only path/hash loses its guard. The original spawned process, offline commands, ownership monitors, rollback and durable retirement must retain descriptor clones until OS-confirmed exit, not merely until their caller or transport ends.

This hook creates no trust, authorization, immutable filesystem or loader-policy grant. Do not create a bundle anchor by hashing a supplied manifest, attach an unrelated state-directory guard, or assume Unix descriptors prevent writes. The legacy CLI/service path currently supplies only a helper path and expected image digest; it must not fabricate a validated bundle/cache lease from those inputs. Wire the guard when the actual validated artifact provider reaches that composition boundary. No new resources-to-transport dependency is needed.

### Limits that callers must not hide

Directory share denial prevents rename, not insertion of new children. A synthetic Windows test confirmed both behaviors. Exact-set revalidation catches an insertion before the final spawn check, but it is not an atomic namespace write lock. Fixed OS imports must use scoped System32 resolution, and helper code must not probe arbitrary app-local DLL names or accept loader paths from untrusted input. Do not claim an attacker-writable namespace is sealed, or that a safe probe alone proves complete native trust. Existing write mappings, administrators, kernel compromise and resource changes after manager death are outside the ordinary handle-sharing guarantee.

Unix read descriptors do not provide Windows deny-write/delete semantics. The verifier uses no-follow descriptor-relative opens, holds original descriptors, rejects aliases and revalidates bytes. `read_verified` returns bytes whose digest matches the external manifest. Strict path execution through `path_for_spawn` and `VerifiedCommand` refuses on Unix rather than pretending an ordinary read FD prevents mutation. Descriptor-backed immutable execution or a proven immutable staging policy needs target-native implementation and acceptance before factories use it. The cross-compiled macOS check is not that proof.

## Trusted build production

The producer is `scripts/rust-package-resource-manifest.mjs`. Its `--trusted-build-output` argument is an explicit operator assertion that the directory came from the trusted build, not a way to make a downloaded directory trustworthy.

```text
node scripts/rust-package-resource-manifest.mjs managed \
  --trusted-build-output ABSOLUTE_FRESH_PUBLISH_DIRECTORY \
  --rid win-x64 --version 0.1.0 --runtime-version 10.0.15 \
  --provenance ABSOLUTE_BUILD_RECORD_OUTSIDE_BUNDLE --probe
```

`--probe` for managed output invokes only `probe`, `probe-isolated` and `probe-power`. It never invokes UAC, `power-guardian`, profile-changing shell launches or global power settings. Go uses only `--probe`. The generator does not execute Darwin or WSL files. Synthetic tests use `probe=false` and mark that absence in provenance.

The package staging code creates these manifests and outside-payload build records before Cargo compiles. It passes their digests through explicit build-only variables:

- `SHELLFOX_WINDOWS_HELPER_MANIFEST_SHA256`
- `SHELLFOX_GO_HELPER_MANIFEST_SHA256`
- `SHELLFOX_DARWIN_HELPERS_MANIFEST_SHA256`
- `SHELLFOX_WSL_SCRIPTS_MANIFEST_SHA256`

`build.rs` validates and embeds supplied values. It never discovers or hashes runtime bundle directories. An absent anchor remains absent. Development callers may instead pass a digest taken explicitly from their trusted build provenance. They must not derive it from an untrusted runtime manifest.

Managed output also contains `managed-inventory.json` and a separate `managed-vulnerability-report.json`. The inventory reads the actual publish dependency/configuration files, including NuGet/runtime-pack versions and managed assembly/file versions. NuGet metadata acquisition is explicit with `fetch-nuget --output ABSOLUTE_SNAPSHOT`. The report preserves source URLs and byte digests, reports unsupported version ranges as incomplete, and says `not-run` when no snapshot is supplied. `SHELLFOX_NUGET_VULNERABILITY_SNAPSHOT` is a build-time package input. `fetch-nuget --output ABSOLUTE_SNAPSHOT --runtime-channel 10.0` also retains official .NET release metadata and reports lifecycle separately. Cargo auditing is a separate report. Neither a clean Cargo result nor no listed NuGet findings proves all native or .NET runtime code is secure or supported.

The historical .NET 8.0.31 scan found 11 matching NuGet advisory records and an EOL channel. That failure remains under `tmp/rust-package-resource-managed-audit/` and was not reclassified as safe.

The native owner remediated the helper to SDK 10.0.404 and runtime 10.0.15 without dropping Windows 10 or ARM64. Rust verification accepted both actual 197-file publishes against their externally supplied x64/ARM64 build-record digests. Repeated metadata audits found zero listed NuGet findings, zero unresolved ranges, an active 10.0 channel and EOL 2028-11-14. Evidence is under `tmp/rust-package-resource-net10-audit/`. These checks do not execute either helper or grant `NoDynamicCodeLoad`. All 67 scoped source imports and disabled startup hooks still do not prove the vendor CoreCLR namespace is closed. Privileged factories remain default-deny pending that proof or a proven private staging contract. NativeAOT is an optional prototype, not an adopted format or permission to drop ARM64.

## Checks

Windows synthetic tests cover DLL tampering with unchanged apphost, substituted manifests, missing runtime, extra/late DLLs, bounded reads, bad paths and capabilities, runtime/RID mismatches, junctions, hardlinks, original-handle write/delete/replacement denial, confirmed-exit lifetime and unconfirmed retention. The child process is a copied Rust test binary, not the installed app or a C# power helper. No UAC or global power activation is part of this suite.

Actual root-workspace Windows x64 tests and strict Clippy passed after the root owner registered member 17. The default suite passed 23 tests with three fixture/prerequisite entrypoints ignored. The Node producer test and the externally anchored actual-bundle test were run explicitly; the latter passed separately for both 197-file builds without execution or a loader-policy grant. The synthetic child entrypoint is exercised by the two lifetime tests. Cross-target checks for macOS arm64 and Windows arm64 passed without linking or executing a target-native helper. Linux/macOS execution and immutable Unix launch remain unverified. Existing release/license gates are unchanged. Nothing was signed, released or installed.
