# Embedded network health

This package is shared by the desktop/headless helper and `native/mobile` through a local Go module replacement. It observes the pinned Tailscale SDK, not the separately installed OS Tailscale service.

`BackendState: Running` is not a connectivity check. Online requires the SDK's control-stream `Self.Online` flag and no typed control/network failure. A stale flag is overridden by `mapresponse-timeout`, `not-in-map-poll` or `network-status` warnings. Unrelated update, logging and configuration warnings do not restart networking. Missing or empty interface enumeration is unknown, not proof that a live mobile connection is offline.

A healthy control stream with a relay/socket problem stays online with a sanitized degraded-health message. Repair refreshes bindings only, so working direct or fallback paths need not be disconnected. Unknown control-plane warnings that may require user action get neutral wording and do not trigger recovery. Raw SDK warning text, URLs and credentials are never forwarded.

## Recovery

One worker reads local SDK status every five seconds and wakes on coalesced NetMon events. It does not scan or probe peers. Thirty seconds of observed, eligible failure triggers an interface refresh and UDP rebind. Sixty seconds of continuous control loss permits `LocalClient.Start` with empty options to rebuild the control client with existing keys and preferences. A transport-only outage cannot shorten that control grace or give a new control outage an old five-minute backoff. Retry delays grow to a five-minute cap. Healthy state resets the policy.

Recovery does not call `Server.Close`, log out, start interactive login, replace listeners, change the proxy URL or secrets, restart the Node host, or touch PTYs. Explicit enrollment and logout share the recovery lock. Mobile shutdown cancels and drains the worker before closing the SDK server.

This is not instantaneous reachability detection. A stale control stream can remain apparently online until the SDK's map-response watchdog or health warning advances. The typed timeout threshold is about two minutes five seconds; SDK warning publication can lag by up to another minute. The recovery grace applies after the failure is observed. OS suspension can delay execution further. No status flag proves that every peer path works. Actual HTTP failure still uses the client's normal reconnect behavior; PTY survival does not guarantee uninterrupted terminal display.

If the local SDK status API is unavailable, the package reports that error and retries the health check. It does not blindly restart an SDK whose identity/authentication state it cannot inspect. It does not recover a crashed helper process. Background networking on Android/iOS remains subject to OS restrictions.

## Evidence

Unit tests cover typed warnings, unknown mobile links, failed recovery, separate transport/control timelines, grace/backoff, coalesced events, single-worker execution and serialized logout. The real SDK fixture uses loopback-only control, DERP and STUN servers. It reproduces Running with an offline control stream, then checks the same node ID, public key, IPs, preferences, original tsnet listener and loopback proxy after retained-identity control restart. It never uses a live tailnet account or changes Wi-Fi.
