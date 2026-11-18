# Opt-in Native002 build

This recipe composes producer candidates for a private experiment. It is not
final recovery acceptance, a release, or a replacement for the existing staged
mobile archives. Native profile 0 is supported by these candidates. Legacy
profile 1 remains explicitly unsupported.

## Inputs and compatibility identity

Apply only these candidates, in this order:

| Order | Checkpoint path under `patches/` | SHA-256 |
| --- | --- | --- |
| 1 | `ansi-export/checkpoints/native-002/0001-ansi-export.patch` | `68d3e2d557594ae91bf4655c6a4fc9bde5a89cbe3958053cb61b0e09cd00183d` |
| 2 | `terminal-profile/checkpoints/native-002/0002-terminal-profile.patch` | `8cc4f2403f4fe69faf1b77916d2db8ac3f713824dbfe273ba714c0a3fdf02d22` |
| 3 | `snapshot-budget/checkpoints/native-002/0003-snapshot-budget.patch` | `28fd8c68edebdd8a014c40123261a2494ea3bde47089b3c2e4dd3cf474012a00` |

The top-level patch files are not substitutes. Preparation checks each candidate
hash and its declared prerequisites, then passes the verified bytes to GNU patch
with forward-only application and fuzz zero. No source overlay or optional patch
selection is accepted.

The engine profile is SHA-256 of `shellfox-native002.recipe`. Its canonical LF
records bind the original source/dependency pins, ordered patches, Zig version,
Rust/Zig target, ReleaseSafe/baseline/default-feature options, app/library versions,
four public header pins and shared Unicode module pin. Output paths and caches
are excluded. The hash is supplied to Zig externally. It does not hash itself.

This is a compatibility identity, not publisher authentication. Original archive
signatures, reviewed patch pins, trusted build tools and artifact provenance are
separate inputs. The prepared artifact manifest detects changed files relative
to that trusted installation; it is not an independent trust anchor.

## Preparation

The ordinary `build` and Cargo defaults remain official vt-1.0.1. Opt in explicitly:

```text
node crates/ghostty-vt-sys/build-support/prepare.mjs verify --experimental-shellfox-native
node crates/ghostty-vt-sys/build-support/prepare.mjs build --experimental-shellfox-native
  --target x86_64-pc-windows-msvc --prefix tmp/native002-install
  --zig <exact-Zig-0.17.0-executable> --patch <GNU-patch-executable>
```

The second command is shown on several lines for readability. The prefix must
not exist. Use a new project-root `tmp/` prefix, never an existing artifact or
staged archive directory. Windows Git installations commonly provide GNU patch
at `C:/Program Files/Git/usr/bin/patch.exe`; pass it explicitly when not on PATH.
No PATH or registry changes are needed.

Preparation uses fresh private source/dependency/cache directories and the
existing child-only Git isolation. It checks the patched source fingerprint
before and after compilation and checks all four installed extension headers.
The normal original archive/version/target checks still apply. Incomplete output
is removed on failure. Original archives are never modified.

The installation includes `shellfox-native002.recipe`, `shellfox-native002.files`
and `shellfox-native002.json`. The JSON records actual headers/library hashes,
source fingerprint and candidate checkpoints, explicitly labeled experimental
and not accepted release artifacts. The expected engine profile is printed.

## Cargo contract

Enable sys feature `experimental-shellfox-native` only with:

- `GHOSTTY_VT_DIR`: the newly prepared project-local installation.
- `GHOSTTY_VT_ENGINE_PROFILE`: its externally expected lowercase 64-hex identity.
- `CARGO_TARGET_DIR`: a private directory below this project's `tmp/`.

The build script rejects old official installations for this feature and refuses
a fork installation when the feature is disabled. It validates the recipe,
target, expected profile, exact public headers and library/import-library hashes
before compiling or staging. `GHOSTTY_VT_LIB_DIR` may not redirect to another
installation. Windows requires its DLL; other experimental targets currently
require static linking. This restriction does not change ordinary build behavior.

After validation, Cargo emits `SHELLFOX_GHOSTTY_ENGINE_PROFILE` for the feature-only
Rust module. The C bridge is compiled only for the feature, with
`SHELLFOX_EXPERIMENTAL_NATIVE=1`. Default builds reference no extension symbols.
The private-output requirement prevents replacing a loaded `target/debug` DLL.
Use child-process environment variables, not global environment configuration.

The runtime binding must still check extension ABI versions, compiled feature
bits and returned profile identity. A caller must not infer authentication,
Legacy support or lossy FullState compatibility from the declared build hash.

## Scoped evidence

The new Node tests cover pinned candidates, prerequisite refusal, deterministic
profiles, source mutation detection and actual ordered application to a fresh
verified source extraction. Rust build-support tests cover exact prepared
manifests and rejection of changed headers/libraries, wrong profiles, duplicate
or missing file records, official installations and non-private output paths.

A fresh private Windows x86_64 MSVC-consumer build succeeded with profile
`162aa6f4890a04c0660d961cd75cf94870b72c3ecdc06d5d9f320becb01aeab1`.
Its public C11 consumer, linked to that same DLL, passed with strict warnings and
reported decode/encode sizes 184/168 and resize offsets 148/132. These results
refer to the new exact headers/library, not older same-size layouts. They do not
claim all upstream tests, other-platform runtime acceptance or a new canonical
mobile artifact build. The producer's broader test evidence remains in its own
candidate checkpoint manifests.
