# Remote connections

Shellfox can connect to another running Shellfox through an embedded Tailscale node. The desktop does not need a separate Tailscale installation, router port forwarding, or an SSH server. Both devices need internet access and must belong to the same Tailscale user in the same tailnet. Tagged devices and cross-user sharing are not supported in this version. Tailnet access policies must permit TCP port 52743 between the embedded nodes.

## Connect

1. Open Shellfox on both machines. In Settings, enable remote access and choose a hostname.
2. Sign in to Tailscale through the browser, or supply a Tailscale auth key. An auth key enrolls a device; a general Tailscale API token is not a substitute. Use an untagged, non-ephemeral key for durable device identity. Organization policies may require administrator approval.
3. On the client, select the other machine and connect.
4. On the host, approve the requesting device in Settings. Approval grants command execution as the user running the host. Approve only devices you trust.

Each machine has its own top-level foldable sidebar section. There is no extra Remote section. Connected machines show their own sessions, terminal profiles, and directories. Clipboard operations and clicked web links use the client machine. Remote directory entry and completion use the host, not the client's file picker.

### Host discovery

Tailscale peers are network devices, not necessarily Shellfox hosts. Desktop available-host lists and `shellfox machines list` include only confirmed Shellfox hosts, like mobile. A normal Tailscale computer, phone or router does not qualify merely because it shares your account or has a similar hostname.

Desktop and headless clients refresh the authenticated same-owner peer map every three seconds. Up to four background probes run at once, each with a two-second deadline. A newly confirmed host pushes a state update immediately. Positive probes are cached for five minutes; negative probes retry after 30 seconds. A device that enables Shellfox after a negative probe waits for that cache interval, then the next poll and probe. Discovery never pairs, sends a client ID or creates an approval request. Older hosts can answer through the same side-effect-free validation fallback used by mobile.

Confirmed stable IDs are saved in `discovered-hosts.json`. Addresses, credentials and approval grants are not cached there. Fresh helper peer identities determine where probes can go. Saved or connecting targets stay visible through temporary peer-map omissions, with offline status, so their status and cancellation controls do not vanish. Legacy saved devices remain unconfirmed until a probe succeeds; saving a device was never proof that it ran Shellfox. `hostConfirmed` is an optional state field for older desktop consumers; current hosts emit it explicitly. The mobile native peer/discovery schema is unchanged.

A peer without a running, remote-enabled Shellfox cannot accept a new Shellfox connection. Cached confirmation identifies a known host, not proof that its terminal service is currently reachable.

## Network health and recovery

Shellfox's embedded Tailscale node is separate from the OS Tailscale service. `Running` alone does not prove that its control connection is online. SDK control-stream state and typed connectivity warnings drive health. A healthy control stream with relay/socket degradation stays online with a warning, so working direct connections need not be dropped.

A single native worker observes local status and coalesced network changes. It first refreshes bindings, then can restart only the SDK control client after sustained control loss, retaining keys, preferences, listeners and proxy configuration. It does not restart the host process or terminals and never replays enrollment keys. SDK stale-stream detection and the recovery grace take time; see the [native health notes](../native/tailscale/nethealth/README.md) for timing and limits.

Mobile preserves pending approval UUIDs during recoverable health errors and resumes them online. Existing authenticated peer streams remain active until their actual HTTP connection fails. Authentication loss still stops those clients. No new discovery probes run while global health is offline.

## Who owns a terminal?

The machine running the terminal owns its processes. A remote client attaches to that terminal rather than launching a replacement shell on every connection.

- Disconnecting a client does not close the host's terminals.
- Quitting the client closes its own local terminals, not terminals on other machines.
- Quitting Shellfox on the host closes that host's terminals after the normal confirmation. The host does not run as a background service.
- Opening Shellfox locally on the host shows the same sessions remote clients use.
- Closing a terminal tab or deleting a session explicitly terminates the affected host terminals.

Each terminal has one controlling client. The first attached client claims it when no one owns it. Other attached clients see a blank terminal with "Another client is controlling this terminal" and a "Take it" button. They receive no terminal contents and cannot type, resize or acknowledge its output. The local desktop has no override.

"Take it" explicitly transfers control. The previous owner becomes a waiting client and its retained terminal contents are cleared. The new owner loads a fresh screen before input and output resume, then fits the PTY to its own viewport. If the owner detaches or disconnects, the longest-waiting attached client is promoted automatically. Control has no expiry or renewal timer. This policy applies across desktop, mobile, CLI, authenticated local host RPC and remote connections.

Terminal requests and live output carry a control epoch so queued work from an earlier owner cannot affect a later owner. Taking control does not start or restart the shell. Current remote clients require the `exclusive-v1` terminal-control protocol; an older host must be upgraded before connecting.

Remote requests have a 1 MiB encoded JSON limit, including protocol overhead. Large environment updates can exceed that limit even when individual values are valid. Such updates are rejected before sending; terminal input accepts the existing 64 KiB frame limit. Reconnect retries use bounded delays. A crashed Tailscale helper requires disabling and re-enabling remote access.

Automatic embedded-network repair preserves the node identity, proxy and listeners without restarting Shellfox or its terminal processes. A control-plane outage can mark discovery offline, but an already-connected peer HTTP stream keeps its terminal view and control. Global health warnings alone do not disconnect it. Actual peer HTTP failure still follows normal reconnection and can require a fresh screen replay, or "Take it" if ownership moved. Authentication loss, explicit disconnect and disabling remote access still stop the view. Terminal-process survival does not guarantee uninterrupted viewing when the data path itself fails.

The host must remain running and awake. Reboot, power loss, host application exit, and process crashes do not preserve running shells. Saved session metadata is not a checkpoint of a running process. Reconnection restores a bounded terminal view, not an unlimited permanent transcript.

## Access and security

Tailscale encrypts the network connection and authenticates device identities. Shellfox separately requires host approval before a device can read sessions or control terminals. Revoking a device removes its Shellfox access without terminating the host's terminals. A new device identity requires new approval.

Pending approval requests remain on the host for one hour after their last request, including while a mobile client's screen is locked. No heartbeat is required during that hour. Approve or Reject resolves the request. Cancel request on the client withdraws its attempt; cancelling an older attempt does not remove a newer request from the same device. Expiration is not rejection, so an active client can retry the same attempt UUID after a lost reply. A full approval queue reports a retryable capacity error rather than claiming the host has queued the request.

The embedded helper accepts network traffic through Tailscale only. Its local connection to Electron uses per-launch secrets and authenticated peer identity. The renderer cannot make arbitrary network requests or invoke an unrestricted host API. Remote operations exclude application settings, SSH credentials, clipboard access, file-manager integration, updates, and administrator terminal creation.

This is terminal access, not a restricted command sandbox. An approved client can execute commands with the host user's normal permissions, read files that user can read, and access secrets available to that user. Tailnet membership alone does not authorize Shellfox access.

Auth keys travel only to the embedded helper for enrollment. Shellfox does not save them as application settings. Tailscale's durable node identity is stored under the Shellfox user-data directory and must be treated as a credential. Its directory and state file are private to the host user. Windows uses a protected current-user ACL without requiring administrator rights; Unix uses directory mode 0700 and file mode 0600. Deleting or replacing that identity changes the device seen by other hosts.

## Building and verification

The Tailscale helper is a separate Go executable, bundled outside ASAR for Windows, macOS, and Linux. Users of packaged builds do not need Go or an installed VPN application. Development builds require Go 1.27.3 or newer and download the pinned Tailscale module dependencies. Set `SHELLFOX_GO` to an absolute Go executable path if it is not on PATH. `pnpm build` builds and probes the native helper; `pnpm test:tailscale` runs its offline tests and protocol smoke. See `native/tailscale/go.mod` and `scripts/tailscale.mjs` for the build configuration.

Tests with fake transport validate Shellfox authorization, routing, delivery, and disconnect behavior. `tests/e2e/remote-connections.spec.ts` starts two real Electron applications and PTYs, checks host approval, quits the client while a remote command continues, reconnects to the same terminal generation, and revokes access. Only the Tailscale transport is simulated in that test. They do not prove that a real tailnet login, NAT traversal, relay connection, or operating-system package works. Release acceptance must exercise two real devices, including approval, revocation, network loss, client restart, concurrent local access, and host quit. macOS and Linux need native acceptance on those operating systems.

The React Native client in `mobile/` uses the same host approval and terminal protocol. It embeds its own outbound-only Tailscale node, so it needs neither the Tailscale app nor a device-wide VPN. Mobile sessions always run on a remote host. See [Mobile](mobile.md) for builds, Android updates, and device verification.
