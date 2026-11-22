#ifndef SHELLFOX_MOBILE_CLIENT_H
#define SHELLFOX_MOBILE_CLIENT_H
#include "shellfox_mobile.h"
#ifdef __cplusplus
extern "C" {
#endif
#define SFM_CLIENT_ABI_VERSION 1
#define SFM_CLIENT_MAX_REQUEST_BYTES 1048576
#define SFM_CLIENT_MAX_RESPONSE_BYTES 8388608
/* Client functions are thread-safe. Terminal create/pump/frame/input/close still
 * belong to the terminal's native serial owner thread, not this client runtime.
 * All pointers are borrowed for the call; NULL is legal only with length zero.
 * Status values 0..6 match shellfox_mobile.h. Additional statuses below.
 * No auth keys, URLs, local filesystem/PTY/helper operations are accepted.
 */
enum { SFM_CLIENT_EMPTY=7, SFM_CLIENT_BUFFER=8, SFM_CLIENT_CANCELLED=9,
       SFM_CLIENT_NETWORK=10, SFM_CLIENT_PROTOCOL=11 };
typedef uint64_t SfmClientHandle;
/* Native must COPY all strings before returning from this callback. They are
 * UTF-8, NOT NUL terminated. Invoke Go Request(peer,path,body) asynchronously.
 * Never block dispatch on the Go request. Complete exactly once using the token
 * below. Native may complete inline. Rust never retains callback or context.
 * deadline_ms is the remaining deadline; max_response_bytes includes envelope.
 */
typedef void (*SfmClientRequestFn)(void *context, SfmClientHandle client,
    uint64_t generation, uint64_t request_id,
    const uint8_t *peer, size_t peer_length,
    const uint8_t *path, size_t path_length,
    const uint8_t *body, size_t body_length,
    uint32_t deadline_ms, size_t max_response_bytes);
typedef void (*SfmClientCancelFn)(void *context, SfmClientHandle client,
    uint64_t generation, uint64_t request_id);
/* Native completion outcomes. Go errors map to UNAVAILABLE, never fake JSON.
 * HTTP success returns raw response JSON including the v1 success/error envelope.
 */
enum { SFM_TRANSPORT_OK=0, SFM_TRANSPORT_CANCELLED=1,
       SFM_TRANSPORT_DEADLINE=2, SFM_TRANSPORT_UNAVAILABLE=3,
       SFM_TRANSPORT_TOO_LARGE=4, SFM_TRANSPORT_AUTHENTICATION=5,
       SFM_TRANSPORT_INVALID_RESPONSE=6 };
uint32_t sfm_client_abi_version(void);
/* peer is an authenticated Tailscale destination ID; client_id is a UUID.
 * Maximum 16 clients/process, 128 live operations/client including queued input.
 * IDs are never reused. Response-space and event-count limits also apply.
 */
SfmStatus sfm_client_create(const uint8_t *peer, size_t peer_length,
    const uint8_t *client_id, size_t client_id_length, SfmClientHandle *out);
/* Starts pair, not a local host. Reconnect invalidates old native request tokens
 * and leases before pairing. No uncertain mutation is retried automatically.
 */
SfmStatus sfm_client_connect(SfmClientHandle client, uint64_t *operation_id);
SfmStatus sfm_client_reconnect(SfmClientHandle client, uint64_t *operation_id);
/* Gracefully withdraw this view via /v1/disconnect before local close. Continue
 * dispatch/completion until its result event, then close. Host terminals survive.
 */
SfmStatus sfm_client_disconnect(SfmClientHandle client, uint64_t *operation_id);
/* Exact shellfox_protocol::RemoteCommand JSON {method,payload}. Unknown fields,
 * unknown methods and invalid payloads fail before any network request.
 * Terminal output is routed to bound native engines, never returned to JS.
 */
SfmStatus sfm_client_operation(SfmClientHandle client,
    const uint8_t *json, size_t length, uint64_t *operation_id);
/* One explicit bounded host long-poll. Repeat after completion, with backoff on
 * network failures. Coarse changed/control events enter the event queue.
 */
SfmStatus sfm_client_poll(SfmClientHandle client, uint64_t *operation_id);
SfmStatus sfm_client_cancel(SfmClientHandle client, uint64_t operation_id);
/* Called from any Rust thread when dispatch or read_event has new work. It must
 * not block or call back into Rust: post a pass to the native owner thread.
 * Wakes coalesce until the next sfm_client_dispatch, so start each owner pass
 * with dispatch. Registering runs one wake; NULL clears it. Close clears it too,
 * and no wake runs after close or a clearing call returns.
 */
typedef void (*SfmClientWakeFn)(void *context, SfmClientHandle client);
SfmStatus sfm_client_set_waker(SfmClientHandle client, SfmClientWakeFn wake,
    void *context);
/* Call on the native owner pass, including after completion/cancel. At most
 * 32 callbacks/call. No request/cancel callback occurs outside dispatch/close.
 * Do not reenter dispatch/close from callbacks. complete/cancel may be called inline.
 */
SfmStatus sfm_client_dispatch(SfmClientHandle client, SfmClientRequestFn request,
    SfmClientCancelFn cancel, void *context);
SfmStatus sfm_client_complete(SfmClientHandle client, uint64_t generation,
    uint64_t request_id, int32_t outcome, const uint8_t *bytes, size_t length);
/* Native-driver copy-out, NOT an unconditional JavaScript event stream.
 * Every JSON event has internal:boolean, including errors. Generic operation,
 * connect/reconnect/disconnect are false. Native ACK/write/resize/poll are true.
 * Native tracks explicit JS operation IDs, but MUST use internal to identify
 * ACK completions/errors because consumed returns no operation ID. Drain all
 * internal ACK/key/resize/poll completions natively
 * for scheduling; suppress success-only internal completions and output-only
 * polls from JS. Coalesce connection/error changes, forward control/changed/
 * reset/reattach/exit state. Deduplicate reset tickets before bridging to JS.
 * No Rust allocation crosses ABI. Sets needed to exact JSON length.
 * EMPTY means no event. BUFFER leaves event queued; retry with adequate capacity.
 * Successful read consumes one event. JSON has operationId/internal/status/result, or
 * event for coarse host changes. No terminal output data in this queue.
 * operationId, connection.connectionId and resetTerminals[] are decimal STRINGS
 * to avoid JavaScript rounding. Protocol-owned sequences remain safe integers.
 * resetTerminals names bindings fenced until the owner-thread reset handshake.
 * resetTickets maps decimal handle strings to never-reused decimal ticket strings.
 * resetGeometry maps handles to {cols,rows}: REQUIRED host parser grid for held
 * recovery, NOT the local viewport. Every initial attachment requires this reset.
 * recoveryReady lists handles whose entire recovery was consumed at that grid.
 */
SfmStatus sfm_client_read_event(SfmClientHandle client, uint8_t *buffer,
    size_t capacity, size_t *needed);
/* Bind BEFORE attachTerminal. tab_id is UUID. A terminal can belong to only one
 * client/tab. Binding is native-only, not a per-frame JS bridge. Unbind rejects
 * pending data and clears the local lease, but does not kill the host terminal.
 */
SfmStatus sfm_client_bind_terminal(SfmClientHandle client,
    const uint8_t *tab_id, size_t tab_id_length, SfmHandle terminal);
SfmStatus sfm_client_unbind_terminal(SfmClientHandle client, SfmHandle terminal);
/* Native terminal owner queries the CURRENT ticket just before reset. EMPTY
 * means a stale coarse event: skip reset. Then call sfm_terminal_reset and
 * reset_done with that ticket on the SAME owner thread. A reconnect between
 * query/reset returns STALE from reset_done: query/reset again. Held recovery
 * cannot install until the matching ticket completes with an empty zero queue.
 * If resetGeometry has a grid, reset MUST use exactly that host cols/rows.
 * reset_done rejects a viewport-sized parser instead of feeding clipped replay.
 * Until done, no output submission/input/ACK is allowed. Only the terminal owner
 * may call reset_done, preventing install between its own query and reset.
 */
SfmStatus sfm_client_terminal_reset_ticket(SfmClientHandle client,
    SfmHandle terminal, uint64_t *ticket);
/* Query the live host grid for EXACT ticket before owner reset. STALE means
 * query ticket again. EMPTY means fence-only reset with no known recovery grid,
 * so keep the existing native grid. Never combine a fresh ticket with stale
 * resetGeometry metadata. Outputs are zero on failure.
 */
SfmStatus sfm_client_terminal_reset_grid(SfmClientHandle client,
    SfmHandle terminal, uint64_t ticket, uint16_t *cols, uint16_t *rows);
SfmStatus sfm_client_terminal_reset_done(SfmClientHandle client,
    SfmHandle terminal, uint64_t ticket);
/* Owner calls after pump before any local viewport fit. Sets 1 only once all
 * recovery bytes entered Ghostty at the host grid, including pending chunks.
 * Until ready, native write/resize return BUSY and cannot reorder a fit before
 * recovery. Desired local viewport is native view state, not resetGeometry.
 * A read-only/blocked view may fit locally after ready, but never resize host.
 */
SfmStatus sfm_client_terminal_recovery_ready(SfmClientHandle client,
    SfmHandle terminal, uint8_t *ready);
/* Call after sfm_terminal_pump. ACK uses the host sequence corresponding to the
 * engine's consumed delivery ticket, never its accepted ticket. BUSY accepts
 * nothing. Queue pressure suspends polling until consumed; no silent drops.
 */
SfmStatus sfm_client_terminal_consumed(SfmClientHandle client, SfmHandle terminal);
/* Feed encoded Ghostty input from sfm_terminal_input here on its owner thread.
 * Shared-client generation/ownership/control epoch checks precede remote write.
 * Write/resize reserve a per-binding FIFO synchronously at C ABI acceptance.
 * At most 64 active+queued intents and 128KiB variable input bytes per binding;
 * exactly one write/resize RPC in flight for that lease. No resize coalescing.
 * Native settlement confirmation is frozen with the typed write at synchronous
 * admission, including omitted vs explicit-null state; dequeue never substitutes
 * a newer settlement token. This does not change the existing input hint policy.
 * BUSY accepts nothing and leaves operation_id zero. Native must retain/retry an
 * unaccepted critical key release before later intents, without shuffling order.
 * Host failure/uncertain mutation, control loss and reconnect cancel successors;
 * accepted uncertain mutations are never retried. ACK/poll/control are separate.
 */
SfmStatus sfm_client_terminal_write(SfmClientHandle client, SfmHandle terminal,
    const uint8_t *bytes, size_t length, uint64_t *operation_id);
/* Additive metadata path. Pass the 0/1 user_initiated hint supplied by the Rust
 * sfm_terminal_input_ex callback, never a platform-local classification. Values
 * other than 0 or 1 fail INVALID with operation_id=0. The hint becomes an explicit
 * wire boolean, including false. Settlement confirmation is still captured only
 * at synchronous FIFO acceptance and never recomputed on dequeue. The legacy
 * write function delegates here with hint=1. Bytes are length-delimited UTF-8;
 * embedded NUL is data, not a terminator. Server policy decides meaningful input.
 */
SfmStatus sfm_client_terminal_write_ex(SfmClientHandle client, SfmHandle terminal,
    const uint8_t *bytes, size_t length, uint8_t user_initiated, uint64_t *operation_id);
/* Uses the same Rust-owned lease as write. Resize the local native engine on
 * its owner thread separately; host dimensions are validated by the protocol.
 */
SfmStatus sfm_client_terminal_resize(SfmClientHandle client, SfmHandle terminal,
    uint16_t cols, uint16_t rows, uint64_t *operation_id);
/* Cancels runtime operations, removes the handle, invokes cancel synchronously
 * for dispatched requests. After return caller may free context. Late native
 * completions return STALE, never invoke callbacks or touch freed userdata.
 * Close does not close bound terminal engines or terminate host terminals.
 */
SfmStatus sfm_client_close(SfmClientHandle client,
    SfmClientCancelFn cancel, void *context);
/* Explicit observer-v2 opt-in. Existing creators, functions, layouts and version
 * numbers above remain v1. This extension is ABI revision 1, WIRE version 2.
 * Profile API flags describe available operations, never server-issued grants.
 * Observer grants delivery only: no input, canonical resize, takeover or upload.
 */
#define SFM_OBSERVER_V2_ABI_VERSION 1
#define SFM_CLIENT_PROFILE_V1 1
#define SFM_CLIENT_PROFILE_OBSERVER_V2 2
#define SFM_CLIENT_PROFILE_MANAGER_V2 3
#define SFM_CLIENT_API_DELIVERY 1
#define SFM_CLIENT_API_INPUT 2
#define SFM_CLIENT_API_HOST_LAYOUT 4
uint32_t sfm_observer_v2_abi_version(void);
SfmStatus sfm_client_profile(SfmClientHandle client, uint32_t *profile,
    uint32_t *api_capabilities);
SfmStatus sfm_observer_v2_create(const uint8_t *peer, size_t peer_length,
    const uint8_t *client_id, size_t client_id_length, SfmClientHandle *out);
/* offer_legacy=0 forbids fallback; 1 permits only a visible legacy offer after
 * canonical discovery checks. No v1 pair/attach/control is automatic. Native
 * MUST use RequestWithStatus and the status completion below for this creator.
 */
SfmStatus sfm_observer_v2_discover(SfmClientHandle client, uint8_t offer_legacy,
    uint64_t *operation_id);
SfmStatus sfm_observer_v2_pair(SfmClientHandle client,
    const uint8_t *attempt_id, size_t attempt_id_length, uint64_t *operation_id);
/* Canonical protocol::observer_v2::Command JSON, the same 43 method names with
 * actual v2 payloads/results. Caller-supplied ACKs are rejected. Input/layout/
 * takeover/upload methods remain recognized but denied by this profile.
 */
SfmStatus sfm_observer_v2_operation(SfmClientHandle client,
    const uint8_t *json, size_t length, uint64_t *operation_id);
SfmStatus sfm_observer_v2_poll(SfmClientHandle client, uint64_t *operation_id);
/* Local fence only; explicitly discover/pair again. Never retries a mutation. */
SfmStatus sfm_observer_v2_reconnect(SfmClientHandle client,
    const uint8_t *client_id, size_t client_id_length);
SfmStatus sfm_observer_v2_disconnect(SfmClientHandle client, uint64_t *operation_id);
/* outcome uses unchanged transport 0..6 values. Success requires actual HTTP
 * status 100..599; transport failure requires status=0 and an empty body.
 * Non-2xx bodies must be empty, matching immutable Go RequestResult. Only a live
 * pending DiscoverV2 404 can become the canonical legacy-offer signal. Other
 * errors never infer status from JSON or English strings. This function is for
 * observer handles only; sfm_client_complete remains the unchanged v1 route.
 */
SfmStatus sfm_observer_v2_complete_http(SfmClientHandle client,
    uint64_t generation, uint64_t request_id, int32_t outcome,
    uint16_t http_status, const uint8_t *bytes, size_t length);
/* Delivery-only native binding. These APIs never construct a v1 Owned/Blocked
 * attachment or input/layout lease. IDs in v2 wire objects remain validated UUIDs
 * and safe integer epochs; native registry/reset IDs remain decimal JSON strings.
 */
SfmStatus sfm_observer_v2_bind_terminal(SfmClientHandle client,
    const uint8_t *tab_id, size_t tab_id_length, SfmHandle terminal);
SfmStatus sfm_observer_v2_unbind_terminal(SfmClientHandle client, SfmHandle terminal);
SfmStatus sfm_observer_v2_terminal_reset_ticket(SfmClientHandle client,
    SfmHandle terminal, uint64_t *ticket);
SfmStatus sfm_observer_v2_terminal_reset_grid(SfmClientHandle client,
    SfmHandle terminal, uint64_t ticket, uint16_t *cols, uint16_t *rows);
SfmStatus sfm_observer_v2_terminal_reset_done(SfmClientHandle client,
    SfmHandle terminal, uint64_t ticket);
SfmStatus sfm_observer_v2_terminal_recovery_ready(SfmClientHandle client,
    SfmHandle terminal, uint8_t *ready);
/* Must run on the engine owner thread AFTER pump consumed/cache acceptance.
 * Recovery ACK is distinct from data ACK. Receiving/enqueuing grants no credit.
 * Clip/pad the fixed canonical grid locally; NEVER call host resize from v2.
 */
SfmStatus sfm_observer_v2_terminal_consumed(SfmClientHandle client, SfmHandle terminal);
/* Common dispatch/cancel/read_event/close above also support observer handles.
 * Observer events add profile:"observer-v2" and use canonical v2 result/fact
 * models, not v1 coercions. connection has status and actual binding|null, with
 * no invented v1 permission/control/layout fields. Replay/data bytes never JS.
 */
/* Native0 Manager uses the existing manager-v2 wire profile and original
 * request-origin/queue-consumption fence. Build this family only with the
 * explicitly matched Native002 SDK artifact. It never changes old creators.
 * API capabilities are not server-issued control/file grants.
 */
#ifdef SFM_ENABLE_MANAGER_V2
#define SFM_MANAGER_V2_ABI_VERSION 1
uint32_t sfm_manager_v2_abi_version(void);
SfmStatus sfm_manager_v2_create(const uint8_t *peer, size_t peer_length,
    const uint8_t *client_id, size_t client_id_length, SfmClientHandle *out);
SfmStatus sfm_manager_v2_discover(SfmClientHandle client, uint64_t *operation_id);
SfmStatus sfm_manager_v2_pair(SfmClientHandle client,
    const uint8_t *attempt_id, size_t attempt_id_length, uint64_t *operation_id);
SfmStatus sfm_manager_v2_reconnect(SfmClientHandle client,
    const uint8_t *client_id, size_t client_id_length);
SfmStatus sfm_manager_v2_disconnect(SfmClientHandle client, uint64_t *operation_id);
/* Canonical manager commands. Caller ACK/write/resize are denied here; native
 * input and consumed functions below freeze actual refs and issue real ACKs.
 * Native attachment metadata removes only recovery.data; no binary bytes JS.
 */
SfmStatus sfm_manager_v2_operation(SfmClientHandle client,
    const uint8_t *json, size_t length, uint64_t *operation_id);
SfmStatus sfm_manager_v2_poll(SfmClientHandle client, uint64_t *operation_id);
SfmStatus sfm_manager_v2_bind_terminal(SfmClientHandle client,
    const uint8_t *tab_id, size_t tab_id_length, SfmHandle terminal);
SfmStatus sfm_manager_v2_unbind_terminal(SfmClientHandle client, SfmHandle terminal);
/* Fresh native decode/install happens on the existing engine owner at pump.
 * There is no ANSI reset dance or caller-supplied recovery acknowledgement.
 */
SfmStatus sfm_manager_v2_terminal_recovery_ready(SfmClientHandle client,
    SfmHandle terminal, uint8_t *ready);
SfmStatus sfm_manager_v2_terminal_consumed(SfmClientHandle client, SfmHandle terminal);
/* Native terminal input JSON or canonical manager Input JSON. Logical IME
 * commit and paste are encoded by the HOST; hardware KeyV2 preserves actual
 * physical IDs. Legacy character-only hardware descriptions reject, never
 * guess a physical key. Actual control/geometry/hints freeze at FIFO admission.
 * user_initiated is the native 0/1 hint; passive/release input remains false.
 */
SfmStatus sfm_manager_v2_terminal_input(SfmClientHandle client, SfmHandle terminal,
    const uint8_t *json, size_t length, uint8_t user_initiated, uint64_t *operation_id);
SfmStatus sfm_manager_v2_terminal_resize(SfmClientHandle client, SfmHandle terminal,
    uint16_t cols, uint16_t rows, uint64_t *operation_id);
SfmStatus sfm_manager_v2_complete_http(SfmClientHandle client, uint64_t generation,
    uint64_t request_id, int32_t outcome, uint16_t http_status,
    const uint8_t *bytes, size_t length);
#endif
#ifdef __cplusplus
}
#endif
#endif
