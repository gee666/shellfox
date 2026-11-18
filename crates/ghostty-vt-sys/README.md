# libghostty-vt build inputs

This crate compiles `adapter.c`, `input.c`, `mouse.c`, `selection.c`,
`colors.c`, `recovery.c`, `presentation.c`, `metadata.c` and `metadata-text.c`
with `cc`, against the official
`include/ghostty/vt.h` public API. It pins libghostty-vt **1.0.1**. Cargo never
downloads Ghostty, invokes Zig, or substitutes a host library for a target library.
The Rust build dependencies are `cc = "1"` and `sha2 = "0.10"`.

## Experimental Native002 extension

The default remains the official library. Feature `experimental-shellfox-native`
requires an explicitly verified, private Native002 preparation and expected
profile. See [the opt-in recipe and limits](build-support/EXPERIMENTAL_NATIVE002.md).
It does not replace signed archives or accepted staged mobile artifacts.

## Windows x86_64

The default for `x86_64-pc-windows-msvc` and `x86_64-pc-windows-gnu` is the official
GNU build. Upstream supports MSVC consumers through its `ghostty-vt.lib` import
library. The adapter uses the Cargo target's C compiler, not Zig's GNU compiler.
Only the DLL is linked, never `ghostty-vt-static.lib`.

`build.rs` checks the retained ZIP's SHA-256 on every build-script invocation,
extracts it into `OUT_DIR`, checks the header and pkg-config versions, then links
`ghostty-vt.lib`. Windows hosts need the built-in `System32/tar.exe`. Cross-build
hosts need `bsdtar` for ZIP extraction, or an explicit prepared installation.

The build copies `ghostty-vt.dll` and `ghostty-vt-notices/` into both the Cargo
profile root and its `deps/` directory. Tests and directly launched executables
can therefore load the DLL without changing `PATH`. This also handles custom
profiles, target triples and `CARGO_TARGET_DIR`. Close running consumers before
replacing a changed DLL. Unchanged DLLs are not rewritten.

A GUI or headless release packager must ship that DLL beside the executable and
include `ghostty-vt-notices/`. Copying only the executable to another directory
is not a complete deployment. This crate does not modify the app packagers.

MSVC can fail to open a nested public header when its full path reaches the
Windows legacy path limit, even when extraction succeeded and the file exists.
The observed isolated release failure had `grid_ref_tracked.h` at about 260
characters. A project-local target directory such as `tmp/rp-win32-x64` reduced
that path by 40 characters. Keep packaging target directories short. On a native
build, omitting a redundant `--target` also avoids Cargo's extra target-triple
path component; cross builds must retain their actual target.

For an explicit short local test directory:

```sh
cargo test --locked --target-dir tmp/vt-win -p shellfox-ghostty-vt
```

Check the complete header path when diagnosing this error, not just `vt.h`.
Do not replace a verified archive or change global Windows registry settings to
hide a compiler path failure. The build script deletes and re-extracts its
`OUT_DIR` installation on every rerun; it does not accept a stale extraction
stamp as proof of a complete installation.

## Retained inputs and explicit acquisition

`build-support/vendor/` retains the original, unmodified archives, about 16.5 MiB
in total, and upstream's `SHA256SUMS` and `SHA256SUMS.minisig`.

| Archive | SHA-256 |
| --- | --- |
| `ghostty-vt-1.0.1-x86_64-windows-gnu.zip` | `8e2eb9e1822e85d786b456c8beb86f49d09c318c91f0c2913f91e7ffa12cda29` |
| `libghostty-vt-1.0.1.tar.gz` | `333dfce97641ef9b536bb9e08907ad86abaaada50e8f7417b70d9284adef37aa` |
| `libghostty-vt-1.0.1-deps.tar.gz` | `91a4e10960a52bf5bf4b15aa2a2cddaff8a89d6cd903d3055bda4fd17de3356d` |

All archives come from
`https://release.files.ghostty.org/libghostty-vt/1.0.1/`, with the filenames above.
The Windows URL is
`https://release.files.ghostty.org/libghostty-vt/1.0.1/ghostty-vt-1.0.1-x86_64-windows-gnu.zip`.
These are the official release inputs, not GitHub's automatically generated tag
archive. The official source archive includes `LIB_VERSION` and builds the VT
library rather than the Ghostty application.

From the repository root, with Node.js 22 or newer:

```sh
node crates/ghostty-vt-sys/build-support/prepare.mjs verify
node crates/ghostty-vt-sys/build-support/prepare.mjs fetch
```

`verify` is offline. It authenticates the upstream checksum manifest and trusted
comment using Ghostty's published minisign Ed25519 key, then checks all three
archive hashes against hardcoded pins. The key is:

```text
RWQlAjJC23149WL2sEpT/l0QKy7hMIFhYdQOFy0Z7z7PbneUgvlsnYcV
```

`fetch` is an explicit network step for missing archives. It verifies bytes
before retaining them. Existing corrupt archives cause an error; remove them
explicitly before fetching replacements. Teams that keep binary archives out of
Git must run this step before Cargo. Keep the signed checksum files in Git.
Cargo itself only needs the pinned Windows ZIP, not Node or the source tarballs.
Cargo verifies the ZIP's pinned SHA-256 rather than rechecking its signature.
Normal Cargo registry acquisition is separate; use a lockfile and a populated
registry cache with `cargo ... --locked --offline` for a fully offline Rust build.

## Offline source builds

Non-Windows targets require a prepared installation. No Zig executable is
installed automatically. Source builds require **exact Zig 0.17.0**, including
its patch version. Development versions and 0.17.1 are rejected.

Acquire Zig separately before building. `build-support/zig-0.17.0.json` pins
archive URLs and SHA-256 values for x86_64/aarch64 Windows, Linux and macOS
hosts, copied from `https://ziglang.org/download/index.json`. Download the
selected host archive into the repository's `tmp/` directory, check its SHA-256
with `Get-FileHash -Algorithm SHA256` on Windows or `shasum -a 256` on Unix,
and unpack it before passing its executable with `--zig`. For example, the
Windows x86_64 archive is
`https://ziglang.org/download/0.17.0/zig-x86_64-windows-0.17.0.zip`, with SHA-256
`b5663f69581dcf391293fbf16c06cb80d81d806545ce618b4d0bab7f0eb8c428`.
The index values were checked over HTTPS. Zig archives were not downloaded or
signature-verified here, and this script never acquires a compiler implicitly.

For example, on Linux:

```sh
node crates/ghostty-vt-sys/build-support/prepare.mjs build \
  --target x86_64-unknown-linux-gnu \
  --prefix ./tmp/ghostty-vt-install/x86_64-unknown-linux-gnu \
  --zig /opt/zig-0.17.0/zig
GHOSTTY_VT_DIR="$PWD/tmp/ghostty-vt-install/x86_64-unknown-linux-gnu" \
  cargo test --target x86_64-unknown-linux-gnu -p shellfox-ghostty-vt-sys
```

The installation path must not exist. The script verifies the source and deps
archives, extracts them under a new repository-root `tmp/` directory, and runs:

```text
zig build --prefix <installation> --system <extracted-1.0.1-deps>
  --cache-dir <fresh-local-cache> -Doptimize=ReleaseFast -Dcpu=baseline
  -Dtarget=<target-specific-Zig-triple>
```

`ZIG_GLOBAL_CACHE_DIR` is also a new, empty directory. The Zig child process has
a Git discovery ceiling at the canonical extraction directory and no inherited
repository-selection variables. Git invoked from the source or dependencies
cannot mistake the enclosing Shellfox checkout for the release tarball's repo.
Existing caller ceiling entries remain in effect. The caller's environment and
Git configuration are not changed. `--system` disables Zig package fetching.
Dependencies stay bundled; the script does not disable SIMD
or optional VT features. It removes its scratch directories on exit and removes
an incomplete installation after failure. It records the Rust target and build
arguments in `ghostty-vt-target.txt` and `ghostty-vt-build.json`. Subprocess
limits are 15 seconds for the Zig version check, 120 seconds for extraction,
and 30 minutes for a source build.

`build --help` lists target recipes for Windows, Linux glibc/musl, macOS, iOS
devices/simulators and Android. Desktop and headless clients use the same recipe.
macOS recipes default to 13.0, iOS to 13.0, and Android to API 26. Override the
Zig deployment triple with `--zig-target` when needed. It must still match the
Rust target's architecture, operating system and ABI. The script cannot prove
that an arbitrary override describes the intended deployment.

Apple builds need an appropriate installed Apple SDK and toolchain. Android
builds require `ANDROID_NDK_HOME` pointing to an exact installed NDK version.
Pin SDK/NDK versions in CI, and configure `cc` and Cargo's linker for cross builds.
On Windows hosts, the adapter converts canonical `\\\\?\\C:\\...` and
`\\\\?\\UNC\\...` paths to ordinary disk/UNC spelling only when passing source
and include paths to the C compiler. Android NDK clang rejects the verbatim
include spelling even when the header exists. Filesystem validation and library
paths remain canonical; no headers are copied or global path policy changed.
The Zig pin and source pins alone do not make external SDK inputs reproducible.
This workflow fixes input versions and build options; it does not claim
bit-identical binaries across different hosts or SDKs.

## Prepared installation override

`GHOSTTY_VT_DIR` selects an installation containing:

- `include/ghostty/vt.h` and `include/ghostty/vt/version.h` for exactly 1.0.1.
- `lib/pkgconfig/libghostty-vt.pc` or `libghostty-vt-static.pc` with `Version: 1.0.1`.
- The selected library in `lib/`, and on Windows `bin/ghostty-vt.dll`.

`GHOSTTY_VT_LIB_DIR` overrides `lib/`, including its `pkgconfig/` directory, and
requires `GHOSTTY_VT_DIR`. Headers still come from the installation root.
`ghostty-vt-target.txt`, when present, must exactly match Cargo's `TARGET`.
External installations without this marker are trusted operator inputs. Header
and pkg-config version checks cannot authenticate their binary contents. Use
`ghostty_version()` in runtime tests to confirm the loaded version.

`GHOSTTY_VT_LINK_KIND` accepts `static` or `dylib`. Windows requires `dylib`.
Other targets default to `static`, which avoids deploying extra shared libraries
on mobile. iOS and Android require `static`. A static installation with external
pkg-config dependencies is rejected rather than silently underlinked. Rebuild
with the bundled dependencies. Linux and macOS shared-library overrides need
application-specific library packaging and loader configuration; this build
script does not add global RPATHs or loader environment variables.

## Typed KeyV2 and opt-in public input probe

`KeyV2`, `PhysicalKey`, `KeyActionV2` and `ModifiersV2` add typed input without
changing the old `Key` fields or the `sf_key` C signature. Both encoder paths
validate physical IDs, action, both modifier masks, Unicode scalar and text
before invoking the shared public encoder. C repeats numeric checks before
narrowing or allocating a public event. All 1024 known modifier patterns survive,
including side-only releases. No modifier-side coherence or consumed-subset rule
is inferred. Layout UTF-8 is borrowed only for the synchronous encode call.

The encode allocation bound is one MiB, not Runtime's 64-KiB write cap. Encoding
returns one owned vector; it does not write, split, or authorize a larger Runtime
write. See the [safe API usage and limits](../ghostty-vt/README.md#additive-keyv2-input).

The default build does not compile the C input probe. To compile it against the
same selected headers/library as the adapter and run its Rust test entry point:

```sh
CARGO_TARGET_DIR=tmp/vt-input-check SHELLFOX_VT_INPUT_V2_PROBE=1 \
  cargo test --locked -p shellfox-ghostty-vt-sys -p shellfox-ghostty-vt --all-targets
```

The enabled build fails if `tests/input_v2_public.c` is absent. The C probe checks
all 176 named public discriminants, C widths, every known active/consumed mask,
all actions and borrowed text/scalar/composition getters. It compares the adapter
with direct public encoder output in six terminal configurations. Rust tests
crosscheck every physical variant name/ID against the compiled C assertions,
exercise V2 and old input behavior, and check allocation bounds and sticky errors.
The probe hook does not change the selected prefix, dependency, default engine,
normal Desktop outputs or any application keyboard-capture path.

## Feed error limitation

Upstream `ghostty_terminal_vt_write` returns `void`. The adapter can report its
own validation failures and callback errors, but cannot detect every internal
upstream parser allocation failure. A successful feed is not a guarantee that
the upstream parser encountered no out-of-memory condition.

## Licenses and checks

`build-support/notices/` contains the upstream MIT license, unmodified
`deps/libghostty-vt-third-party.json`, and dependency license texts from the
pinned source/deps archives. It includes build-only aro as well as the linked
projects Highway, simdutf, translate_c, uucode and Wuffs. uucode's supplemental
Unicode and Hoehrmann notices come from its exact upstream commit because the
deps archive omits the referenced license directory. `notices/README.md` records
their provenance. The original archives retain their remaining source notices.
Source installations receive
these notices under `share/ghostty-vt-notices/`. Distributors must retain the
notices for linked third-party code, not just Ghostty's root MIT license.

Run acquisition/build-plan tests with:

```sh
node --test crates/ghostty-vt-sys/build-support/prepare.test.mjs
cargo test -p shellfox-ghostty-vt-sys --locked --offline
```

The Cargo `build-support` integration-test target uses `build-support/install.rs`
and `sha2 = "0.10"` as a dev dependency. Its 22 Windows tests cover installation validation, corrupt archive rejection,
DLL staging, compiler-path conversion and experimental manifest/header/library
admission. Path coverage includes Unicode, spaces, UNC roots and preservation of
device paths. Fixture directories live
under the repository-root `tmp/` directory and are removed when each test ends.

`.github/workflows/ghostty-experiment.yml` runs the signed-input checks,
preparation tests, sys/wrapper tests in debug and wrapper tests in release,
Clippy, workspace formatting and a desktop `cargo check` on Windows x86_64
MSVC. It is experimental build coverage, not a release or UI acceptance job.
Linux and macOS CI are not configured yet because these source builds have not
been validated. Mobile source builds also need pinned SDK inputs and runners.

Locally verified: official archive hashes, both minisign signatures, Windows
x86_64 MSVC adapter compilation and import-library linking. A fresh Android
arm64/API 24 source build with pinned Zig 0.17.0 and NDK 27.1.12297006 passed,
followed by the Rust wrapper's locked Android cross-check using that installation
without copied headers or an extra include-path workaround. The preparation tests
also exercise real Git ancestor isolation. This is build evidence, not Android
device/runtime acceptance. Linux, macOS, iOS, Windows GNU and Windows ARM64
execution remain unverified here. A source recipe is not a tested platform claim. Missing
inputs, a wrong Zig version, failed builds and unsupported recipes stop with
errors; no placeholder library is generated.
