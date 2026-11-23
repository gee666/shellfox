# Active Windows helper runtime and loader review

## Shipped runtime and build toolchain

The active helper targets `net10.0-windows`, self-contained runtime `10.0.15`.
SDK `10.0.404` builds both `win-x64` and `win-arm64`. The active test project also
uses .NET 10. Test-only packages are Microsoft.NET.Test.Sdk 18.12.0, xunit 2.9.3,
and xunit.runner.visualstudio 4.0.1. None of those test packages ships with the
helper. The actual publish dependency graph contains the helper project and the
matching Microsoft.NETCore.App.Runtime RID pack, not the test graph.

The .NET 8 channel is EOL as of 2026-11-10. Its latest patch 8.0.34 does not make
that channel acceptable for shipping. The retired native test projects remain
net8.0 and use the preexisting 8.0.34 test runtime only. They are not dependencies
of the active helper and are not published into its bundle. The exited-process
identity fixture now starts only absolute System32 cmd.exe with `/d /c exit 0`,
a cleared environment and an owned temporary cwd. It no longer runs PowerShell.

Provision from Microsoft's official release JSON and verify its archive SHA-512
before extracting or executing anything:

```text
node scripts/dotnet-sdk.mjs provision
node scripts/dotnet-sdk.mjs test
node scripts/terminal-native.mjs --arch x64
node scripts/terminal-native.mjs --arch arm64
```

The SDK archive, SDK installation, CLI home, caches and evidence stay under
project `tmp/`. No global install, global.json, PATH or user settings change.
`SHELLFOX_DOTNET` may name an explicit absolute dotnet.exe with the exact pinned
SDK version. An absent/mismatched SDK fails rather than finding another one on
PATH. This resolver applies only to the active helper/test build calls. Legacy
projects retain their existing toolchain selection. The Windows CI resource
build provisions the same project-local SDK, including on native Arm64 runners.

Sources:

- https://builds.dotnet.microsoft.com/dotnet/release-metadata/10.0/releases.json
- https://builds.dotnet.microsoft.com/dotnet/release-metadata/8.0/releases.json

At review, channel 10 is active, latest runtime 10.0.15, EOL 2028-11-14. The pinned
x64 SDK archive SHA-512 from the release JSON is
`330ddd2665b43cd2e428216a0d8ea1ac7bb19b5434fdc530e62010dc3b52823a620bf95489d628e1ed6f5122401793ee4887cca4a019122361a1231497f632ee`.
This is SDK download provenance, not a hardcoded runtime bundle trust anchor.
Native Arm64 provisioning selects and verifies the separate official Arm64 ZIP.

## Windows minimum comparison before retargeting

The official current .NET 8 and .NET 10 Windows sections are identical:

- https://raw.githubusercontent.com/dotnet/core/main/release-notes/8.0/supported-os.md
- https://raw.githubusercontent.com/dotnet/core/main/release-notes/10.0/supported-os.md

Both list Arm64/x64 and Windows 10 Enterprise 1809, along with older Enterprise
1607. Both include Server 2019 and the same listed Server/Core editions. The
helper still rejects Windows builds below 10.0.17763 before mode dispatch.
ConPTY already required that Windows 10 1809/Server 2019 generation. System32
import scoping does not raise that minimum. No newer OS API or framework UI
component was introduced by retargeting, and no Windows target was dropped.
Vendor OS-edition lifecycle limits still apply. A supported-OS table is not a
promise that an EOL Home/Pro release has vendor support.

Provisioning retains both full source documents, verifies matching Windows
sections, and records the build-17763 guard. SDK metadata/OS evidence is under
`tmp/dotnet10/evidence/`. This is documented compatibility, not a native test on
every old Windows x64/Arm64 machine. Arm64 publishing here is cross-compilation.

## Application loader review

All 39 application P/Invokes have a per-method
`DefaultDllImportSearchPaths(DllImportSearchPath.System32)` attribute. Their
library names are fixed literals: kernel32.dll and advapi32.dll. This covers
ConPty, LocalPipe and PipeRun. No assembly-wide attribute changes unrelated code.
A compiled reflection test checks every reviewed import and its search policy.

The helper source has no Assembly.Load/LoadFrom/LoadFile, AssemblyLoadContext,
NativeLibrary, Reflection.Emit, LoadLibrary/GetProcAddress, DLL search-path
mutation, plugin resolver, or payload-selected managed/native library name.
The source regression check covers all active C# files. JSON fields carry data;
PipeRun's explicit child executable and the fixed System32 PowerShell are
separate process launches, not library loads in the helper. These checks
supplement human source review, not a generic proof about arbitrary future C#
code.

The published runtimeconfig disables `System.StartupHookProvider.IsSupported`.
The .NET 10 implementation and feature-switch definition are in:
https://raw.githubusercontent.com/dotnet/runtime/v10.0.15/src/libraries/System.Private.CoreLib/src/System/StartupHookProvider.cs

The trusted launcher must still remove DOTNET_, COMPLUS_, CORECLR_, COR_ and
DEVPATH injection/override variables before spawn. A managed entry point cannot
sanitize native profiler or host override code that loads before that entry
point. The Rust launcher's environment scrub and helper-directory cwd remain
necessary. Standard CLR loading of declared framework dependencies is not an
arbitrary extension/plugin API.

The optional [NativeAOT prototype](NATIVEAOT.md) is a separate, opt-in build of
the same source. It is not a reason to relax the managed artifact gate. Its own
native loader/provenance, license and ARM64 requirements remain unresolved.

## Actual inventory and advisory audit

Audit the actual published `.deps.json` and `.runtimeconfig.json`, not merely the
project's package list. Self-contained runtime packs do not appear as ordinary
PackageReference entries. The observed x64 and Arm64 graphs each contain the
matching Microsoft.NETCore.App.Runtime.win-* 10.0.15 pack and the helper project.

Official NuGet vulnerability metadata returned zero matching findings and zero
unresolved ranges for those actual graphs. Official .NET release metadata
reported active/current-in-listed-channel. Test-project transitive NuGet audit
also reported no listed vulnerabilities. The evidence records metadata source
URLs and digests. This is a time-bounded advisory/lifecycle result, not a claim
that no runtime or OS vulnerability exists.

Packaging regenerates the managed inventory and vulnerability report for every
architecture and refuses to package on findings or an unsupported channel.
Packaging notices come from the exact runtime 10.0.15 tag.

Allowed checks execute only the safe probe and ordinary owned process fixtures.
They do not run UAC, legacy self-test or an account/profile process. No
release, signing, installation or production-data migration is part of this
remediation.
