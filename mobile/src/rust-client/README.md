# Rust-backed mobile presentation

`useShellfox` uses the native V1 client on Android through `client/factory.android.ts`.
`TerminalView.android.tsx` uses the Ghostty Fabric view without a WebView fallback.
The same screens still handle sessions, files, settings and approval. Embedded Go
owns authenticated tailnet transport. The factory checks the Rust module ABI,
required methods and Fabric registration before connecting.

On iOS, the WebView stays the default until the native V1 simulator check passes.
To exercise the native V1 path on a simulator, launch with `--shellfox-native-v1`.
For an isolated tailnet pass `--shellfox-control-url http://127.0.0.1:<port>` too.
ManagerV2 remains a separate experimental build and launch profile.

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

Run device checks for drawing, IME, selection, file links, resize and reconnect.
JavaScript and C ABI fixtures do not exercise those platform interactions.

Focused checks include the factory, RustHostClient, RustTerminalView, existing
TerminalView, SessionScreen and useShellfox suites. Test native modules use
inert fixtures only; no account data or network is needed.
