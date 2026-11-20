# Observer-v2 mobile source integration

This is a separate, explicit delivery-only facade. `useShellfox` defaults to
native v1. The
`createProfileClient` returns a discriminated profile/client result; an observer
is not a v1 HostClient and cannot feed invented SessionDto rows to legacy screens.
The native module remains `NativeShellfoxRustClient`.

The canonical contract is `crates/shellfox-protocol/observer-v2.md`. Source tests
parse and serialize all 24 goldens and all 43 method request/response examples.
An opt-in cross-language test also reads actual C ABI results from the Rust fixture
harness. Set `SHELLFOX_CAPTURE_OBSERVER_CABI=1` for the canonical 43-method Rust
test, then pass its unique root-tmp file as `SHELLFOX_OBSERVER_CABI_CAPTURE` to
`cabi-capture.test.ts`. It independently loads goldens and checks the real result,
handshake and native recovery ACK. It does not trust a capture's expected values,
filter Python fields or alter fixtures. Only the documented native carrier
projection is allowed: binding is in connection metadata, and recovery bytes stay
native. The reader is skipped without the explicit flag, bounds input to 16 MiB,
and refuses files outside `tmp/observer-v2-crosslang`. Remove only owned captures
after the joint pass. This is test-only, never a general runtime capture facility.

Numeric JSON formatting is normalized. Omission, explicit null, known absence,
unknown/not-applicable facts, unavailable catalog entries and source-end versus
original-process exit remain distinct. No creation times or Python paths are
filled from current settings, the local clock, or v1 presentation defaults.

## Explicit profile and lifecycle

The observer factory requires the existing native gate plus extension ABI 1.
The wire version is 2, not ABI 2. `clientProfile` checks a CLIENT handle, never an
engine handle. Profile 1/API bits 7 and observer profile 2/API bits 1 describe
available APIs, not current server grants. Both-platform acceptance remains a
separate prerequisite. The current source work does not meet that gate.

Creation performs only local native registration/profile checking. Discovery,
pairing with an explicit attempt ID, polling, withdrawal and reconnect are named
operations. There are no hidden pairing/mutation retry loops. Pending accepted
operations have bounded deadlines and cancellation. `reconnect` is a local fence
only; it does not discover, pair, attach or choose legacy. Native `closeClient`
performs bounded graceful withdrawal before local destruction, including pending
attempts. Closing an engine uses `closeTerminal` and never queries client profile.

Only Rust discovery can return a visible `legacyAvailable` result. The facade
does not inspect HTTP status or English error messages to infer it, never starts
a v1 client automatically, and rejects that offer after v2 pair admission.
Native transport must use immutable Go `RequestWithStatus` results and pass the
actual status to the new completion ABI. Only a live pending DiscoverV2 404 can
participate in the canonical fallback decision. Approval, auth, timeout and
business UNSUPPORTED responses cannot be translated into that signal.

## Passive terminal ownership

`ObserverSurface` exposes configuration, attachment metadata and cleanup only.
It has no input, takeover, upload, resize or caller-ACK methods. Binding passes no
local columns or rows. The native owner must apply the exact canonical recovery
grid before feeding bytes and must clip/pad on view/font changes, not resize or
locally fit the parser. Common Rust engine input and viewport-resize entry points
also reject observer-bound engines; owner-thread recovery reset remains allowed.

Recovery bytes and high-volume base64 data never enter the facade event stream.
Native attachment metadata strips recovery data and retains the actual recovery
stamp. Native consumption drives distinct recovery/data ACKs. No receipt,
JavaScript layout event or caller assertion grants credit. Query replies and
clipboard effects from the passive mirror are discarded. SourceKind metadata
never grants input, layout or original-source cleanup authority.

## Verification boundaries

TS fixtures/mock native transports use no accounts, HTTP, Go runtime or native
processes. They do not establish HTTP-status propagation, source permissions,
real native consumption ACKs or platform acceptance. The isolated Rust role test
uses an inert observer with no discover/pair/dispatch and verifies common engine
input/resize rejection.

After coherent C ABI and native source gates, this extension requires matching
fresh four-ABI Rust artifacts and regenerated gomobile bindings. Old staged v1
archives remain tied to their original manifests. They must not be relabeled as
observer-v2 builds. iOS native compilation remains blocked when Xcode is absent.
