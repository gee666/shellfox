# Optional Windows NativeAOT build

The administrator helper can be built with NativeAOT. An x64 build works and passes the checks below. Releases still ship the managed self-contained build. Switching would first need an ARM64 build, a complete review of what the native image loads, and cleared native toolchain licenses. The Rust packaging has no resource kind for an AOT helper.

## Commands

```text
node scripts/terminal-native.mjs --arch x64
node scripts/terminal-native.mjs --aot --arch x64
node scripts/terminal-native.mjs --aot --arch arm64
```

The first command is the managed build and writes `tmp/terminal-native/win-x64`. AOT writes to `tmp/terminal-native-aot/win-x64`, with its own intermediate and bin caches, and never overwrites managed output. The probe and the launch schema are the same in both builds. There is no extra runtime CLI command or raw command-line field.

The AOT profile sets `PublishAot`, turns off reflection-based JSON, uses invariant globalization and treats compiler, trimming and AOT warnings as errors. Invariant mode avoids loading ICU. Protocol text stays full UTF-8, and the child shell's environment and the user's locale are unchanged.

JSON output uses explicit JSON DOM fields and primitive values instead of runtime type discovery. Tests compare every request and response shape byte for byte with the previous serializer, including escaping, Unicode, integer widths and several cultures. Validated `JsonElement` forwarding still uses the original raw text, which keeps large apostrophe frames intact. Lifecycle, authentication, UAC, Job and rollback code are shared with the managed build.

## Prerequisites and minimum Windows

The project-local SDK is 10.0.404. The embedded runtime, NativeAOT runtime pack, ILCompiler and ILLink packages are 10.0.15. The build records the restored package versions and NuGet hashes rather than reading them from the EXE name.

References:

- https://learn.microsoft.com/dotnet/core/deploying/native-aot/
- https://learn.microsoft.com/dotnet/core/deploying/native-aot/cross-compile
- https://learn.microsoft.com/dotnet/core/deploying/native-aot/interop
- https://learn.microsoft.com/cpp/build/reference/dependentloadflag

Microsoft supports Windows x64 and ARM64 NativeAOT targets. Cross-architecture publishing needs the matching VS2022+ C++ tools and libraries, not just an ARM64 RID. .NET 10 still supports the helper's minimum, Windows build 17763. The dependent-load flag needs build 14393, which is older, so the minimum doesn't change.

The development machine this was built on has VS2022 17.5 (MSVC 14.35) x64 and Windows SDK 10.0.22000 x64 and ARM64 libraries, but no MSVC ARM64 tools or static CRT. The ARM64 AOT command fails before replacing any artifact. The script installs nothing and doesn't change PATH. `SHELLFOX_MSVC_ROOT` and `SHELLFOX_WINDOWS_KITS_ROOT` can name absolute tool roots; the tool search and `LIB` changes apply only to the build process.

That x64 toolset is old. A successful link says nothing about its support status, security or redistributable licenses. Shipping would need a supported toolset and the ARM64 components. Managed ARM64 publishing works without them.

## What the native image loads

The image has no CLR directory, managed DLL, deps or runtimeconfig. It is compiled code, not a single-file bundle that extracts a CLR. The build invocation, package inventory and the image itself show this; the audit doesn't assume that any EXE without a CLR header is AOT.

All 68 application OS imports keep fixed names and System32 attributes. NativeAOT can turn known imports into direct PE imports, which ignore those attributes, so the AOT profile also links `/DEPENDENTLOADFLAG:0x800`. The PE audit checks that field, the x64 or ARM64 machine type, the missing CLR directory, NX and ASLR, and every direct and delayed import. Only the listed OS libraries and UCRT API sets pass. Dynamic P/Invokes keep their System32 policy, and the managed build's loader restrictions stay in place.

The native image can't load managed assemblies or startup hooks late, which helps a long-running helper. A matching probe, hash or build JSON still isn't enough to ship it. That would need a typed record of the trusted build, a review of static and lazy imports, native-runtime license inputs and an ARM64 build.

## Checks

```text
node scripts/dotnet-sdk.mjs test
node --test scripts/terminal-aot.test.mjs scripts/terminal-aot-probe.test.mjs
```

The probe output matches the managed build. The native image passes the PE audit. The NativeAOT, ILCompiler and runtime package graphs have no listed NuGet advisories and are on a current, supported .NET 10 channel. That audit doesn't cover MSVC, the Windows SDK or native code in general.

The first CMD PipeRun check failed with status 1 and "path not found" (`tmp/aot-evidence/cmd-pipe-initial-failure.json`). `CommandLine` now leaves only nonempty, conservative ASCII argument tokens unquoted. The executable is always quoted, `Quote` is unchanged, and empty, Unicode, whitespace, quote and shell-metacharacter arguments keep the previous encoding. There is no raw CLI field or shell fallback. Generic argv round trips passed, and then the System32 CMD `/d /q /c` check returned its exact marker and exit code 23.

A generic-argv Node fixture failed RNG initialization with a fully empty environment. It now gets only `SystemRoot` and `WINDIR`, with no account hooks or credentials. The CMD fixture still uses an empty environment and `/d`, and runs no profile or script file.

The output keeps the .NET runtime and NativeAOT package licenses and notices, compiler, package, source and image hashes, the linker response hash, the PE audit and a separate NuGet and lifecycle report. The native CRT and SDK licenses haven't been reviewed, and the .NET notices don't cover them.

No UAC prompt or real elevated terminal has run with the AOT build. It hasn't been built for ARM64, released, signed or installed.
