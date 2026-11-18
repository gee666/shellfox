# Rust-backed mobile presentation

The existing `useShellfox` model now calls `client/factory.ts`. Its default is
explicitly `webview-transition`, which constructs the unchanged TypeScript
remote client and WebView terminal. Module detection never changes that choice.

The alternate `rust-native` path constructs `RustHostClient`, which implements
the same public client interface and inherits the existing `RemoteOperations`.
Sessions, files, settings, discovery, approval presentation and background
lifecycle reuse the current React Native screens. Embedded Go still owns the
authenticated remote request pipe. The Rust path never falls back to direct
JavaScript networking, a local host, helper process or PTY.

## Selecting an engine

`setMobileClientEngine(selection)` is an explicit application configuration
entry point for newly created clients. Do not change it behind a connected view.
The Rust choice must include acceptance evidence for Android and iOS. The factory
also checks module ABI, required methods and Fabric registration. Missing support
throws a reason for the existing UI error path. It never switches to a WebView
after a user has selected Rust. Until both platforms pass acceptance, leave the
default transition selection in place.

No module lookup, pairing, polling, credentials or native engine creation occurs
when constructing a transition client. Native components load lazily only for a
registered Rust terminal binding, so old binaries do not import an enforcing
native component during startup.

## Native client and terminal flow

`RustHostClient` starts only when the existing model calls `connect()`. Typed
operations use the shared request and response schemas, including chunked file
transfer, directories, stats and terminal uploads. A bounded operation pump reads
coarse native results and deduplicates connection notifications. It never reads
terminal output. Completion origin is explicit through `internal: boolean`;
internal ACK/input/resize successes are not presentation events.

Each Rust client registers a terminal driver by its stable `terminalApi` object.
`TerminalView` uses that registration to choose `RustTerminalView`, without
changes to App or SessionScreen props. The driver creates, configures and binds
an opaque native terminal handle before attaching. Attachment results contain
metadata only. No JavaScript code fabricates empty replay chunks or feeds raw
output into the view.

The native view owns IME, hardware input, gestures, output parsing, rendering,
lease-checked writes, resize and consumption credit. Existing TypeScript
accessory keys send physical identity and layout text for Ghostty to encode.
Sticky modifiers, keyboard toggles, safe-area layout, theme, paste preview,
selection/copy and upload controls remain in the existing presentation.
Read-only automation views keep selection but cannot send input or take control.

Suspend stops the old client cycle. Terminal cleanup waits for in-flight
initialization, detaches, unbinds and closes handles. Native `closeClient` must
perform bounded graceful disconnect before local destruction. Resume creates a
fresh client identity; native generation fences and owner-thread reset prevent
old callbacks from feeding or ACKing new recovery.

## Recovery acceptance

The current native client can use the existing host's bounded ANSI transition
contract. A host that cannot supply its negotiated terminal restore format must
return UNSUPPORTED. The UI shows that failure and does not switch engines or
invent restoration. Full-state recovery and native platform parity remain
acceptance requirements, not claims established by source integration.

Native drawing, IME, OS minimums, selection, file-link navigation, settlement
races, ordered input under load, recovery geometry, reconnect and lifecycle
cleanup still need platform/runtime evidence before adoption.

Focused checks include the factory, RustHostClient, RustTerminalView, existing
TerminalView, SessionScreen and useShellfox suites. Test native modules use
inert fixtures only; no account data or network is needed.
