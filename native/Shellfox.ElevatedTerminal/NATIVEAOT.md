# Optional Windows NativeAOT prototype

Feasible, with an x64 prototype built and checked. This is not an accepted Rust
resource kind or a shipping-profile change. Managed self-contained publishing
remains the default. NativeAOT acceptance is blocked until ARM64 build evidence,
complete loader review and all native license/toolchain gates are resolved.

## Scope and commands

The same .NET 10 project supports an opt-in profile:

```text
node scripts/terminal-native.mjs --arch x64
node scripts/terminal-native.mjs --aot --arch x64
node scripts/terminal-native.mjs --aot --arch arm64
```

The first command remains managed and writes `tmp/terminal-native/win-x64`.
AOT writes to `tmp/terminal-native-aot/win-x64`, with separate intermediate/bin
caches. It never overwrites managed output. Mac/Linux helpers are unchanged.
No new runtime CLI command or arbitrary raw command-line field was added.
The probe and the launch schema are unchanged.

The profile sets `PublishAot`, disables reflection JSON, enables invariant
helper globalization and treats compiler, trimming and AOT warnings as errors.
Invariant mode avoids optional ICU selection. Protocol text remains full UTF-8;
it does not change the child shell's environment or the user's OS locale.

JSON emission now uses explicit JSON DOM fields and primitive values, not
runtime type discovery. Tests compare every response/request shape against
the previous serializer byte-for-byte, including escaping,
Unicode, integer widths and multiple cultures. Validated JsonElement forwarding
still uses the original raw text, preserving large apostrophe frames.
Lifecycle, authentication, UAC, Job and rollback code were not replaced by an
AOT-specific implementation.

## Prerequisites and OS floor

The project-local official SDK is 10.0.404; the embedded runtime, NativeAOT
runtime pack, ILCompiler and ILLink packages are 10.0.15. Actual restored package
versions and NuGet hashes are recorded, not inferred from the EXE name.

Official references:

- https://learn.microsoft.com/dotnet/core/deploying/native-aot/
- https://learn.microsoft.com/dotnet/core/deploying/native-aot/cross-compile
- https://learn.microsoft.com/dotnet/core/deploying/native-aot/interop
- https://learn.microsoft.com/cpp/build/reference/dependentloadflag

Microsoft documents Windows x64 and ARM64 NativeAOT targets. Cross-architecture
publishing requires the corresponding VS2022+ C++ tools and libraries, not just
an ARM64 RID. The existing .NET 10 Windows support matrix still covers the
helper's build-17763 floor. The dependent-load flag needs Windows 10 RS1, build
14393, which is older than that existing floor. No OS target was dropped.

This host has VS2022 17.5/MSVC 14.35 x64 and Windows SDK 10.0.22000 x64/ARM64
libraries. Its MSVC ARM64 tools/static CRT are absent. The ARM64 AOT command
fails explicitly before replacing any artifact. No global installer, toolchain
update or PATH change is performed. `SHELLFOX_MSVC_ROOT` and
`SHELLFOX_WINDOWS_KITS_ROOT` may select explicit absolute trusted tool roots;
tool search/LIB changes apply only to the build child.

The installed x64 toolset is old. Successful linking is not a claim that its
support/security status or redistributable license inputs are cleared. Obtain
supported toolchain evidence and the ARM64 component through an approved route
before shipping. Managed ARM64 publishing continues to work independently.

## Native loader proof

The resulting image has no CLR directory, managed DLL, deps or runtimeconfig
payload. It contains compiled application/runtime code, rather than a managed
single-file bundle that extracts a CLR. This fact is backed by the trusted AOT
build invocation/package inventory and the actual image, not a heuristic that
any EXE without a CLR header is AOT.

All 68 application OS imports retain fixed names and System32 attributes.
NativeAOT can turn known imports into direct PE imports, which ignore those
attributes. Therefore the AOT profile also links `/DEPENDENTLOADFLAG:0x800`.
The bounded PE audit verifies the emitted field, expected x64/ARM64 machine,
absent CLR directory, NX/ASLR and every direct/delayed import. Only the reviewed
OS libraries and UCRT API sets are accepted. Dynamic application P/Invokes keep
their existing System32 policy. Managed artifact loader denial stays in place.

The native profile removes late managed assembly/startup-hook loading. That
simplifies the long-lived guardian case, but does not authorize a resource kind
from a matching probe, hash or mutable build JSON. A typed trusted-build record,
static/lazy import review, native-runtime license inputs and ARM64 gate are still
required. No Rust resource/factory code was changed here.

## Evidence and checks

```text
node scripts/dotnet-sdk.mjs test
node --test scripts/terminal-aot.test.mjs scripts/terminal-aot-probe.test.mjs
```

The safe probe was compared against managed output. The actual native image passed
the PE policy audit. Actual NativeAOT/ILCompiler/runtime package graphs reported
no listed NuGet findings and an active/current supported .NET 10 channel.
That audit is not an MSVC, Windows SDK or whole-native-code security clearance.

The original owned CMD PipeRun check failed with status 1 and a path-not-found
message. Its record is `tmp/aot-evidence/cmd-pipe-initial-failure.json`. It was
not counted as a pass. With explicit approval, CommandLine now leaves only
nonempty conservative ASCII argument tokens unquoted. The executable is always
quoted, Quote itself is unchanged, and empty/Unicode/whitespace/quotes/shell
metacharacters retain the previous encoding. No raw CLI field or shell fallback
was added. Generic argv round trips passed before rerunning CMD. The owned
System32 CMD `/d /q /c` check then returned its exact marker and exit code 23.

A separate generic-argv Node fixture initially failed RNG initialization with
an entirely empty environment. It now supplies only SystemRoot/WINDIR, without
account hooks or credentials. The CMD fixture still uses an empty environment
and `/d`; it executes no account profile or script file.

The output retains .NET runtime license/notices, the actual NativeAOT package's
license/notices, compiler/package/source/image hashes, linker-response hash,
PE audit and a separate NuGet/lifecycle report. A native CRT/SDK license review
is explicitly pending, never implied by the retained .NET notices.

No UAC or real elevated terminal was run. The AOT x64 prototype is not platform parity, ARM64 runtime
acceptance, a release, signing or installation.
