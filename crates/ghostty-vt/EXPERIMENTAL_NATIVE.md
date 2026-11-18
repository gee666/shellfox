# Opt-in verified Native002 bindings

The Cargo feature `experimental-shellfox-native` adds checked operations to the existing `Terminal`. It does not change the default application engine, expose Legacy support, or claim universal FullState.

Default builds do not compile the extension module, reference its new symbols or stage a fork DLL. Enabling the feature requires Heather's explicitly prepared private Native002 installation and exact expected profile. The build checks the candidate recipe, ordered patch/input hashes, four public headers and library; an official ZIP is not an experimental fallback.

```text
GHOSTTY_VT_DIR=<verified private installation>
GHOSTTY_VT_ENGINE_PROFILE=<exact producer compatibility profile>
CARGO_TARGET_DIR=<isolated project tmp directory>
cargo test --locked -p shellfox-ghostty-vt --features experimental-shellfox-native --test extension_native
```

The build emits `SHELLFOX_GHOSTTY_ENGINE_PROFILE` as a validated compile-time expectation. `VerifiedNativeExtension::verify()` then checks the linked library's exact ABI versions, compiled features, snapshot format, profile bytes and named width policy. A runtime caller cannot replace the compiled expectation with another identity. Compatibility identity is **not authenticity**; source/artifact verification remains a separate producer responsibility.

## Existing ownership, not another parser

The private C bridge borrows a public Ghostty handle only while the existing Engine is exclusively borrowed. Atomic encoding uses one `&mut Terminal` borrow and one C State/body capture call. It never drains source query replies or metadata.

Constructor/decoder handles remain in a private RAII guard until adoption. The C adapter allocates its render/key/mouse storage before the transfer commit:

- Success returns one SfVt, clears the caller's owned handle once, and transfers it to the existing Engine destructor.
- Failure clears the output, retains caller ownership and frees partial adapter resources.
- No Rust code casts or assumes the SfVt layout.
- Adoption never calls `sf_new`, feeds/replays a source or changes decoded defaults, cursor, modes or continuation tracking.
- No userdata, PTY, clipboard, title, bell or metadata callbacks are registered on an adopted passive terminal. Metadata peek returns no value and acknowledgement is invalid for its null metadata store.

The original adapters retain their current effect-capture behavior. The shared storage helper preserves their allocation order. Passive feature-only status checks reject a native semantic failure rather than publish an apparently valid frame.

## Explicit configuration and limits

`RestorationConfig` has no default. Callers must declare exact `runtime_image_storage_limit` and `runtime_resize_pull_scrollback`; direct-only image IO is fixed by this Native002 binding. The native atomic encoder checks actual source configuration. Decode applies the captured configuration before continuation. Neither a build hash nor constructor defaults are substituted for source policy.

`SnapshotLimits::default()` is a finite admission policy, not live configuration. Callers can reduce its budgets, including zero. Zero never means unlimited. Binding hard caps bound caller allocations as well as the C codec's separate budgets:

- Encoded body/caller buffer at most 16 MiB.
- C cumulative reservation budget at most 512 MiB.
- Deterministic work budget at most 100,000,000,000 units.
- Dimensions at most 4096; visible engine grid remains at most 1,048,576 cells.
- Continuation at most 64 KiB.

The defined wire profile has zero image/decompression admission and no history recompression. Runtime image storage is a different explicit setting. Work units are not an OS CPU-time quota; allocation reports are not process RSS and exclude caller buffers, stacks and library code. Live input and later resizing remain separately bounded application operations.

## Snapshot records

`encode_native_snapshot` returns owned `NativeSnapshot` plus a checked budget report. Metadata contains engine identity, wire/library/format versions, canonical cut State and exact restoration configuration. It never serializes C struct padding. The encoder captures State internally; a separate getter is not used to manufacture the cut.

`NativeSnapshot::from_untrusted` copies only bounded input and validates its metadata. The body remains untrusted until every `decode` invokes the bounded Native002 decoder. Missing State, wrong identity/version and Legacy are rejected. A terminal is published only after FINISH/exact EOF, report/State/grid checks and successful adoption. No READY terminal or partial replacement escapes on failure.

Reports are checked for exact size/version, zero reserved data, checked heap-plus-mapped arithmetic, allocation/work bounds and source offset. Encoder failures must invalidate State and report zero written bytes. Decoder failures must return no terminal. Private staging ownership closes all validation and transfer error paths.

## Fresh passive ANSI targets

`export_native_ansi_fresh` returns `NativeAnsi`, not generic FullState. The extension preflights its accepted Native profile and returns unsupported-state reason bits instead of silently losing state.

`NativeAnsi::restore` constructs and configures a fresh passive existing `Terminal` internally. There is no safe convenience method that applies the record over an arbitrary current terminal. The canonical reset colors/cursor/mode baseline is checked by the native exporter; sources with different reset defaults can fail explicitly. Scrollback and runtime configuration are declared and configured on the fresh target before input.

ANSI records are bytes. Their final continuation may be incomplete UTF-8 and must not be converted lossily into an old v1 JSON string or fed to a legacy client under another capability label.

## Scope and evidence

The tested private installation reports compatibility identity `162aa6f4890a04c0660d961cd75cf94870b72c3ecdc06d5d9f320becb01aeab1`. This is an opt-in Native002 producer, not a final accepted public engine identity. Staged old mobile/official libraries and signed input archives are unchanged.

The integration tests use the existing Terminal/render/selection APIs and actual private DLL. They cover atomic capture, profile verification and Legacy refusal; failed transfer then retry; decoded continuation/default preservation; alternate buffers, live suffix and resize-pull false/true; source query/metadata noninterference; passive effects; malformed/truncated/trailing bodies; metadata mismatches; zero budgets and retry; and fresh-only ANSI reconstruction. Separate tests reject malformed FFI State/report values before publication and assert exact current C layouts/offsets.

Current Windows MSVC verification against the private matched DLL:

- Feature-enabled wrapper/sys all-targets: 102 passed, with the separate DOM-browser prerequisite test ignored by default.
- New safe integration target: six passed in debug and release.
- Default official-library regression: 91 passed; the extension target has zero compiled tests/symbol calls without the feature.
- Feature-enabled Clippy with `-D warnings`: passed.
- Root/mobile `pnpm typecheck`: passed.
- Enabling the feature without explicit private installation/profile: rejected by the build, with official fallback disabled.

These checks do not verify other-OS runtime behavior, application delivery, Legacy layout2 or universal terminal fidelity. New compiled dependency/header/API changes require a fresh matching host/desktop/mobile acceptance build. Do not relabel old staged archives as these bindings or use the opt-in test identity as production authorization.
