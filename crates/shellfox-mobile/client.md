# Native shared client

`include/shellfox_mobile_client.h` is the remote-only companion to the terminal
header. Its implementation uses `shellfox_client::Client<NativeTransport>` and
`ViewCursor`, not a second wire client. The shared protocol's sealed request
implementations validate outgoing methods and decoded responses. There is no
local host, shell launch, PTY, helper-process, login, URL or auth-key operation.

## Native driver

1. Create a client with an authenticated Tailscale destination ID and client UUID.
   Create the terminal engine on its native serial owner thread. Bind its tab UUID
   with `sfm_client_bind_terminal` before submitting `attachTerminal`.
2. Call `sfm_client_connect`. Run `sfm_client_dispatch` regularly on a native
   worker. Its request callback copies peer, path and body before returning, then
   schedules the existing Go `Request(peerId, path, body)` away from the owner
   thread. Events and ordinary requests need separate concurrent Go workers.
3. Return raw Go response JSON through `sfm_client_complete` with the exact
   client/generation/request token. Go errors map to `SFM_TRANSPORT_UNAVAILABLE`.
   They must not become an invented success envelope. The Go API does not expose
   HTTP status or per-call cancellation, so Rust does not assume either exists.
4. Read completed operations through the copy-out event API. A zero-capacity
   read returns `SFM_CLIENT_BUFFER` and the required size without consuming the
   event. Allocate native storage, read again, then release that native storage.
   Rust never exports a buffer to free. This is a native-driver completion queue,
   not an unconditional JavaScript event stream. Apply the filter below.
5. Submit protocol `RemoteCommand` JSON as `{ "method": ..., "payload": ... }`.
   This is the existing protocol enum, not another set of mobile DTOs. Unknown
   fields, unknown methods and local-only operations fail before transport.
   Generic operations call shared `Client::execute_command`, whose single match
   selects a typed request and validates its typed response before erasing it
   for presentation. Mobile keeps native terminal attachment/detach, ordered
   mutation admission, lease/ACK and upload cursor checks. It does not duplicate
   the shared method allowlist.
6. Run one explicit `sfm_client_poll` at a time. Repeat after its result. Respect
   `connection.retryAfterMs` and permissions. Rejected/approval-required results
   are decisions from the host, never claims supplied by the mobile application.
   The generic mobile command entry point rejects `writeTerminal` and
   `acknowledgeTerminal` even with a valid current lease. JavaScript cannot inject
   raw terminal bytes or grant output credit through that channel.
7. Terminal output goes directly from validated Rust responses to the bounded
   native terminal queue. Attach results omit replay bytes; data events do not
   enter the coarse event queue. Pump and render on the terminal owner thread.
   Call `sfm_client_terminal_consumed` after pump, retrying BUSY after operation
   completion even if no new pump occurred. EMPTY means no new credit is due.
8. Encode validated input with `sfm_terminal_input_ex`. Forward its nonempty
   borrowed byte span and Rust-derived 0/1 hint unchanged to
   `sfm_client_terminal_write_ex`. Do not classify it again in JavaScript, Kotlin,
   Objective-C or C++. The callback runs after engine borrows and queue locks
   have been released, so it may synchronously enter the native client FIFO.
   Legacy `sfm_client_terminal_write` remains a native compatibility entry point
   that supplies hint 1. Use
   `sfm_client_terminal_resize` for the remote resize and resize the local engine
   on its owner thread. Both remote operations derive the current generation and
   control epoch from Rust's cursor. Native code must not construct a lease.
9. For graceful withdrawal, dispatch `sfm_client_disconnect` through completion,
   then `sfm_client_close`. Immediate close is local-only cancellation. Neither
   operation kills host terminals. Close terminal engines separately on their
   owner threads.

Native callback functions and their context are borrowed only by `dispatch` or
`close`. Rust does not retain either. Dispatch and close serialize per client;
close waits for an active callback to return before invalidating the handle.
Callbacks must not reenter dispatch/close, throw, or unwind. Completion and
cancellation may run inline or from another thread. Closing cancels Rust waits;
its native cancellation callback must fence a Go job that cannot actually abort.
Late native results return STALE and never touch a retired callback context.

The same operation entry point includes the protocol's file chunks/transfers,
image/asset operations, terminal uploads, directory operations and session stats.
Their validated results may cross to the existing JavaScript file presentation.
File chunks are at most 192 KiB before base64 encoding, with whole request/response
and event-space limits unchanged. This permission for file content does not apply
to terminal output, recovery bytes or cells. The adapter does not open a local
file, invent an upload DTO, retry an uncertain write, or bypass host permissions.

## JavaScript event filter

Every completion event has `internal: boolean`, on both success and failure.
`sfm_client_operation`, connect, reconnect and disconnect produce `false`.
Native consumed ACKs, terminal write/resize and background polls produce `true`.
The origin belongs to the entry point, not the method name. A generic UI resize
is explicit, while the dedicated native resize is internal. The generic command
channel has no raw-write or ACK capability. Input metadata uses additive `_ex`
functions; the legacy native function signatures are unchanged.

The native driver also keeps a set of operation IDs launched for explicit JavaScript
requests. Insert an accepted operation's ID on the native serial driver before
that driver can drain its completion. Do not insert IDs for native ACKs, encoded
keys, native resizes or background polls. Remove an explicit ID when its completion
is delivered, and clear that client's set when it closes. IDs are decimal strings
at the JavaScript boundary, but may remain `uint64_t` in native storage.

Use the `internal` tag rather than ID membership to classify native completions
and failures. ID filtering alone cannot identify ACK failures because
`sfm_client_terminal_consumed` does not return an operation ID.

Drain every Rust event natively. Internal completion events release scheduling
state and permit ACK retries or another poll, even when nothing crosses to JS.
Forward a correlated explicit operation's completion/error to its JavaScript
caller. For internal operations, process and retry failures natively, then forward
only actual coarse state changes:

- Compare the connection status, permission, connection ID and retry/error state
  with the last delivered state. Repeated metadata is not another JS event.
- Forward changed/activity/control/exit/error/reattach events from the poll result. An empty
  `result.value.events` array is an output-only poll completion, not a JS event.
- Deduplicate reset instructions by the current reset ticket, not handle alone.
  The owner still queries the live ticket before resetting, as described below.
- Coalesce internal transport or application failures into error state. ABI
  `status != 0` and shared-protocol `result.ok == false` both need inspection.
  Raise one coarse critical error when the error/connection state changes, not
  a JS operation completion on every frame. Clearing that error is a state change
  too. Never discard an internal error before native retry/state handling.

Successful internal ACK, key and resize completions do not cross to JS. The native
driver must not emit their operation IDs or synthesize an output/render event for
each poll. Terminal bytes, delivery tickets and rendered cells remain native.
Rust retains internal completions because dropping them here would prevent the
native driver from knowing when backpressure or a failed ACK can be retried.

## Reset and reconnect

Reconnect cancels old operations, replaces the shared client's view UUID, changes
its transport generation and invalidates every terminal cursor before pairing.
No uncertain mutation is resent. A successful pair also changes the shared
connection ID. Every attachment, including the first one, requires an owner-thread
reset before any recovery byte can enter Ghostty.

Operation events contain `resetTerminals`, an array of decimal handle strings,
and `resetTickets`, a map from handle strings to never-reused ticket strings.
Before acting on any reset event, the native terminal owner calls
`sfm_client_terminal_reset_ticket`. EMPTY means that an old event can be ignored.
Otherwise query `sfm_client_terminal_reset_grid(client, terminal, ticket, ...)`.
A successful grid query supplies the exact host parser columns and rows.
Reconnect invalidation clears the old grid along with the old cursor and held
recovery, so a fence-only reset cannot be mistaken for a new host-grid replay. Reset
with those dimensions, then call
`sfm_client_terminal_reset_done(client, terminal, ticket)` on that same owner
thread. EMPTY from the grid query means a connection fence with no known host
recovery geometry; keep the existing grid for that queue-clear step. STALE from
either query or completion means query and reset again. Never combine a new live
ticket with dimensions copied from an older event.

Events also include `resetGeometry`, keyed by decimal terminal handle, for native
state presentation. Those are host dimensions, not the desired local viewport.
Rust verifies the matching ticket, owner thread, empty queue, zero progress and
actual Ghostty dimensions before installing held recovery. An 80x24 viewport
cannot consume a 200x80 recovery until its parser has been reset to 200x80.
All Ghostty reads/resets here use the terminal owner, never Tokio.

After reset, pump all recovery chunks at the host grid. Query
`sfm_client_terminal_recovery_ready` after pump before fitting the local viewport.
It returns ready only when the final recovery delivery ticket was consumed,
including chunks initially held by queue pressure. Coarse completions also carry
a `recoveryReady` handle list. Input, remote resize and polling stay blocked until
this parser barrier passes. ACK can progress independently. Only then may the
native owner fit locally and request a controlling host resize through the input
FIFO. Store desired viewport dimensions separately from recovery geometry. A
blocked/read-only view must never resize the host.

Held recovery and its reset ticket prevent old queued output from being parsed
after the new recovery or credited to a new generation. The mirror discards terminal query replies; it must
not send them back to a host that already answers them.

## Ordered input and upload fencing

Native write and resize calls reserve a per-binding FIFO slot synchronously at C
ABI acceptance, before spawning any Tokio future. The same FIFO accepts explicit
typed UI resizes, but rejects generic raw-write commands. One worker sends exactly one mutation at a time for
that lease and waits for its result before sending the next. Write, resize, key
release ordering therefore does not depend on task polling or native Go workers.
There is no resize coalescing and no reordering around writes.

The native write's settlement confirmation is captured from `ViewCursor` under
that same synchronous admission lock. The queued typed request retains that exact
token even if a newer manual settlement arrives before send. Dequeue checks the
lease but never substitutes a newer settlement token. Omitted, explicit null and
timestamp are distinct states throughout serialization. The `_ex` client entry
point validates hint 0/1 and sends an explicit boolean, including false. Rust's
validated input variant supplies the hint; the client does not infer it from bytes.
The host's meaningful-input policy remains authoritative. Embedded NUL is valid
length-delimited input, including Ctrl+Space; it is never a C-string terminator.

Attach acceptance also marks the binding synchronously. Settlement events arriving
while its RPC or owner reset is pending stay in a bounded per-generation buffer.
The replacement cursor observes matching events before input is released, so an
older snapshot or a late old-generation event cannot erase a newer confirmation.
Attachment presentation metadata comes from the actual typed response with only
replay chunks removed; optional fields are not collapsed into null values.

Each binding admits at most 64 active-plus-queued intents and 128 KiB of variable
input bytes, including the active request. BUSY leaves the operation ID zero.
Native must hold and retry an unaccepted critical release before accepting later
input. An accepted mutation is never retried. A write the host refuses with
an ordinary error (validation, rate, not ready) drops only that input, as 0.9.15
did. A lost control or terminal, an uncertain result, or any refused resize
fences the lane and cancels its successors; the completion event then carries a
`reattach` list, and JS reattaches as for a reattach poll event. The fresh
attachment opens a new lane and its fit sends the current size again. Control
loss, detach, reset, reconnect and close fence active tokens and clear queued
successors. ACK, poll and control operations
do not join this FIFO. At most four input lanes send concurrently per client,
leaving shared-request capacity for non-input work; a blocked lane does not hold
another lane's turn or a polling/ACK barrier.

The four terminal-upload operations use the shared `ViewCursor::begin_upload`
and `UploadCursor`. A binding tracks at most four upload IDs from successful host
begin responses. Chunk offsets, completion ACKs, generation, control epoch and
connection must match the stored cursor. Unknown IDs and caller-forged identities
are rejected before Go. Only one upload mutation is active per ID; offsets advance
only after an exact positive chunk ACK. Failure makes that upload uncertain, with
no automatic chunk retry. Cleanup can use only the original recorded fenced
identity, never a caller's replacement epoch.

## Credit and bounds

Native delivery tickets are not host output sequences. Every queued output chunk
records its native ticket and host sequence. Only the engine's **consumed** ticket
can produce a candidate ACK. Accepted or merely buffered data grants no credit.
The candidate comes from `ViewCursor::consumed` on a clone. One ACK is in flight
per binding. Only a successful host ACK commits the actual cursor and acknowledged
watermark; failures release the flight for retry without another pump. Transport
failure can require shared-client reconnect and recovery before more traffic.

The existing protocol bounds terminal data chunks at 16 KiB, below the engine's
256 KiB chunk maximum. Attachment replay totals at most 256 KiB. A host cannot
smuggle a larger native chunk through a valid protocol response. The native queue
is bounded by 4 MiB and 64 chunks. A full queue retains the remaining validated
batch in Rust and blocks another poll. Ticket metadata is bounded even if a caller
pumps without reporting consumption. Nothing silently drops output on BUSY.

Whole outgoing envelopes are limited to 1 MiB and responses to 8 MiB. The adapter
checks lengths before copying and enforces request deadlines/cancellation with
Tokio, even if Go never completes. It admits at most 16 clients, 32 bindings per
client, 128 live operation slots and 256 queued-plus-reserved result slots.
Response-space reservations keep queued-plus-reserved event storage within 32 MiB
per client. Generic operations reserve a full bounded response, so only three
can be admitted concurrently at the current 8 MiB response limit. Typed write,
resize and ACK completions reserve 16 KiB each instead, allowing the bounded FIFO
without blocking ACK admission behind generic response reservations. The native
queue and at most one deferred poll batch are separate bounded storage.

Opaque handles, operation IDs, reset tickets and connection IDs in event JSON
are decimal strings. Wire-owned output sequences/control epochs are validated
JavaScript-safe integers. Never turn an opaque ID into a JavaScript number.

## Verification

No account, production host or network is used by these commands:

```sh
cargo test --locked --offline -p shellfox-mobile --lib
cargo clippy --locked --offline -p shellfox-mobile --all-targets -- -D warnings
```

The client contract tests call the exported C ABI with a mock native-Go callback,
feed the shared client's v1 fixtures, and parse output through the real Ghostty
engine. They cover typed envelopes, approval/revocation, malformed/oversized
payloads, cancellation/deadlines, close racing dispatch, bounded admission and
output queues, lease-controlled input/resize, ACK-after-consume, failed ACK retry,
reconnect fencing, input FIFO/release ordering, settlement admission freezing and
attach races, upload cursor fencing and host-grid
recovery with split CSI and native UTF-8 chunks. These are host-side Rust tests.
Linkage, the Go executor, rendering, IME, suspend/resume and the owner-thread
lifecycle still need checks on devices.
