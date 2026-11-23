# Mobile Rust bindings

Remote-only Rust client and Ghostty terminal bindings for React Native 0.88.0.
There is no local host, PTY, helper process, loopback HTTP server, credential
store or login operation in this crate. The existing embedded mobile Go
`Request(peerID,path,body)` implementation supplies authenticated tailnet routing.

Android and iOS use this native client and the Ghostty Fabric terminal, with the
same session, file and settings screens. The client factory checks the module ABI
and Fabric component first. A missing capability reports an error; there is no
fallback terminal.

## Native ownership

- Terminal ABI: [include/shellfox_mobile.h](include/shellfox_mobile.h).
- Remote client ABI: [include/shellfox_mobile_client.h](include/shellfox_mobile_client.h).
- Input JSON: [schema/input.schema.json](schema/input.schema.json).
- React Native codegen specs: `mobile/src/rust-client/specs/`.
- Android implementation belongs in `mobile/android/`; iOS in `mobile/ios/`.

Create, pump, render, input, selection, reset and destruction run on one stable
OS thread. Use an Android HandlerThread or an iOS NSThread. A serial GCD queue
can change OS threads and does not satisfy the Ghostty owner rule. Rust keeps
engines in thread-local storage and shares only a bounded output queue. Handles
are monotonic integers, never raw pointers. Owner exit drops its engines and
invalidates their IDs. Engine faults poison further operations until reset.

Native transport workers copy output into Rust. The queue accepts contiguous,
one-based local tickets, at most 256KiB/chunk, 64 chunks and 4MiB total. BUSY
accepts nothing and does not advance a ticket. Each pump parses at most 512KiB.
Credit advances only after Ghostty parses a whole chunk. The shared client maps
local consumed tickets back to remote sequence/control epochs before ACK.
A split remote chunk receives credit only after its final native segment.

A renderer consumes borrowed frame cells on that owner thread and copies any
data it needs to dispatch to the UI thread. No cell, frame, terminal output or
network payload travels through JavaScript to render the screen. JavaScript
receives coarse lifecycle, operation and connection state. It must keep handles,
operation IDs, sequence IDs and connection IDs as decimal strings.

The platform keyboard keeps a small editor (the text since the last Enter or
key) so suggestions, swipe, voice and autocorrect work. After every keyboard
edit it reports the editor's committed text; Rust's composer sends the terminal
backspaces back to the first changed character and the new text. Marked text
stays a local preedit. Keys, mouse and paste go through Ghostty's encoders.
Native forwarding uses the additive \`sfm_terminal_input_ex\` callback and sends its
validated 0/1 gesture hint unchanged to \`sfm_client_terminal_write_ex\`. Key
release and button-free mouse motion are passive. Drag, click, wheel and nonempty
text/paste are active. Editor resets and empty encoded results emit no callback. Native
and JavaScript never duplicate this classification. The host still applies its
meaningful-input policy before clearing settlement. The client captures the
tri-state settlement token when its bounded FIFO accepts the write, not on dequeue.
Encoded bytes can include NUL for Control-Space; every native copy uses the supplied
length. Neither callback receives a NUL-terminated string.
The host already answers terminal queries; the mobile mirror drains and discards
its own replies. Selection copy does not write the clipboard. Hyperlinks are
untrusted text, not permission to open a URL. Cursor shape and blink are not
currently exposed by the safe wrapper; native renderers use a steady block.

## Reconnect and recovery

Fence the old client generation and lease before allowing new terminal data.
Invalidate old native callback tokens, tickets, pending output and ACKs. On the
owner thread, call `sfm_terminal_reset` to replace the parser, clear selection,
drop queued bytes and reset local credit to zero. A successful reset requires
`sfm_client_terminal_reset_done` before installing held attachment/recovery.
Never reset from Tokio or an arbitrary transport worker. Never feed old queued
bytes after ANSI recovery, or ACK an old generation using new tickets.

Callbacks in the terminal header are synchronous. Their pointers are borrowed
only until return; copy before crossing threads. Native callbacks must not throw,
retain pointers or recursively mutate terminal state. Input callbacks run after
all engine borrows and queue locks have been released. They may query progress,
frame or mouse tracking and enter the client FIFO on the owner thread. Other
terminal callbacks remain nonreentrant. Errors are numeric statuses with static,
sanitized messages. Rust never returns an allocation that native code must free.

Client request dispatch also borrows strings only during callbacks. Native copies
them, starts Go requests asynchronously, and completes exactly once with the
generation/request token. Rust retains no callback or context pointer. Closing
cancels requests synchronously through the supplied cancellation callback; late
completions return STALE. The Go API has a bounded 30-second deadline and no
per-request cancellation hook, so native cancellation must suppress completion
delivery and release tokens without claiming that Go's underlying request stopped.

## Codegen

The package hook names `ShellfoxRustSpecs`, `type: all`,
`jsSrcsDir: ./src/rust-client/specs`, Android package `com.shellfox.mobile`.
iOS providers map `NativeShellfoxRustClient` to `ShellfoxRustClient` and
`ShellfoxRustTerminal` to `ShellfoxRustTerminalComponentView`.
Specs use public React Native 0.88 exports, not legacy deep-import declarations.

Generate and validate with the installed pinned codegen:

```sh
node node_modules/@react-native/codegen/lib/cli/combine/combine-js-to-schema-cli.js tmp/mobile-rust-codegen/schema.json mobile/src/rust-client/specs
```

The typed adapters validate input and progress. Constructing them never connects,
copies auth state, polls or selects the native terminal automatically. Use explicit
fixtures/mock transports for tests. Do not copy production account data.

## Reproducible builds

The recipe pins Ghostty vt-1.0.1 and Zig 0.17.0 through the existing verified
source-build helper. It explicitly overrides that helper's Android API26 default
with API24. Android keeps minSdk24 and NDK27.1.12297006 on arm64, armv7, x86 and
x86_64. iOS reads the installed RN0.88 `min_ios_version_supported` helper and
checks that the Podfile uses it. It currently resolves to 15.1. Do not lower a
platform requirement or raise the application minimum to make a build pass.

```sh
node crates/shellfox-mobile/build-mobile.mjs plan --target aarch64-linux-android
node crates/shellfox-mobile/build-mobile.mjs build --target aarch64-linux-android --zig /path/to/zig-0.17.0
node crates/shellfox-mobile/build-mobile.mjs plan --target aarch64-apple-ios
```

Install the Rust targets, exact NDK, Zig and Apple SDK explicitly first.
Build mode never fetches archives, installs tools or changes authentication.
Use a fresh prefix under `tmp/mobile-rust-build/<target>/ghostty` for every build.
Archive provenance and licenses remain in the Ghostty sys crate. Link the target's
`libshellfox_mobile.a` into native JNI/ObjC++ adapters with the platform system
libraries. Android shared output is `libshellfox_mobile.so`; JNI code is supplied
by the Android integration, not this crate. Apple targets include arm64 device,
arm64 simulator and x86_64 simulator. Package their static libraries separately
as an XCFramework; never lipo device and simulator arm64 into one archive.

## Verification and remaining gates

```sh
cargo test --locked --offline -p shellfox-mobile --all-targets
cargo clippy --locked --offline -p shellfox-mobile --all-targets -- -D warnings
pnpm exec vitest run mobile/src/rust-client/adapter.test.ts mobile/src/rust-client/remote-adapter.test.ts
node --test crates/shellfox-mobile/build-mobile.test.mjs
```

These tests don't cover drawing, wide cells, selection and copy, hyperlinks, IME
composition, hardware keys, mouse mode, resize, reconnect, output pressure or
lifecycle cleanup. Check those on a device or simulator for both platforms.
