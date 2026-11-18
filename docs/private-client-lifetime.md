# Experimental private-client lifetime

This is the Rust experiment's local transport, not a remote/mobile method or a native UI capability. It uses the same private OS endpoint, verified peer identity and endpoint secret as ordinary local RPC. There is no TCP fallback or second metadata owner.

## Strict framing

The private endpoint record includes `transportVersion: 1`. An older/mismatched record is rejected, not treated as permission to start another host.

Frames retain the existing bounded length prefix. JSON is strict and rejects unknown fields/variants. No frame type implements Debug.

Opening a client lifetime:

```text
{kind:"lifetimeOpen", version:1, secret, clientId}
→ {kind:"lifetimeOpened", version:1, clientId, epoch}
```

The server creates a fresh epoch and binds it on the existing owner actor before replying. The opening response uses the same fresh response-consumption trailer/echo as ordinary requests. The client then retains that original connection until disconnect/drop/process exit. No client reader task or transport clone keeps the connection alive after the client is dropped.

Ordinary requests:

```text
{kind:"request", version:1, secret, clientId, epoch, body:[bytes]}
```

The inner typed envelope's client ID must equal the outer ID. The actor checks the exact live epoch again immediately before dispatch. The existing application namespaces and six automation method constants are unchanged; lifetime opening is a transport frame, not an application method.

## Authority and EOF

A replacement lifetime releases the previous UUID binding first. A later request receives a new monotonically allocated controller. Existing raw runs retain their original controller, so the same UUID with a new epoch cannot read/write/adopt the old pipes or reopen stdin. An old connection's EOF compares its exact epoch and cannot revoke a replacement.

Actual EOF closes only that client's stdin/raw collection/input leases. It does not kill the raw process or detach unrelated clients. There is no release after each RPC and no automatic reconnect or mutation replay. The raw manager's bounded read-credit timeout remains a separate backpressure policy, not the implementation of disconnect.

## Bounds and shutdown

- At most 16 request/header tasks.
- At most 128 authenticated lifetime owners, with separately bounded permits/watchers so they cannot occupy every RPC slot.
- Actor lifetime/controller tables are bounded, and stale epochs fail before mutation.
- Normal Stop retains the original actor/ownership guard through pending response-consumption receipts, then cancels/drains lifetime watchers. Abnormal transport shutdown cancels waiters without replaying queued operations; original resource cleanup remains owned by the same guard.
- Controller EOF cleanup uses typed pre-enqueue Full handling, not error-message matching. A lost actor/ownership is handled by its original global cleanup owner.

Owned tests cover same-client deferred stdin/output progress, 128 lifetime capacity, stale dispatch, replacement isolation, actual CLI death causing natural target EOF before the 30-second lease timeout, and a fast CLI target exiting while caller stdin remains held open. These Windows tests do not establish missing native process-ownership support on other platforms.
