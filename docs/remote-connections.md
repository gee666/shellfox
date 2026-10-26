# Remote connections

Shellfox can connect to another running Shellfox through an embedded Tailscale node. The desktop does not need a separate Tailscale installation, router port forwarding, or an SSH server. Both devices need internet access and must belong to the same Tailscale user in the same tailnet. Tagged devices and cross-user sharing are not supported in this version. Tailnet access policies must permit TCP port 52743 between the embedded nodes.

## Connect

1. Open Shellfox on both machines. In Settings, enable remote access and choose a hostname.
2. Sign in to Tailscale through the browser, or supply a Tailscale auth key. An auth key enrolls a device; a general Tailscale API token is not a substitute. Use an untagged, non-ephemeral key for durable device identity. Organization policies may require administrator approval.
3. On the client, select the other machine and connect.
4. On the host, approve the requesting device in Settings. Approval grants command execution as the user running the host. Approve only devices you trust.

Each machine has its own top-level foldable sidebar section. There is no extra Remote section. Connected machines show their own sessions, terminal profiles, and directories. Clipboard operations and clicked web links use the client machine. Remote directory entry and completion use the host, not the client's file picker.

Tailscale peers are network devices, not necessarily Shellfox hosts. A peer without a running, remote-enabled Shellfox will not accept a Shellfox connection.

## Who owns a terminal?

The machine running the terminal owns its processes. A remote client attaches to that terminal rather than launching a replacement shell on every connection.

- Disconnecting a client does not close the host's terminals.
- Quitting the client closes its own local terminals, not terminals on other machines.
- Quitting Shellfox on the host closes that host's terminals after the normal confirmation. The host does not run as a background service.
- Opening Shellfox locally on the host shows the same sessions remote clients use.
- Closing a terminal tab or deleting a session explicitly terminates the affected host terminals.

Local input always remains available. Remote clients share a renewable 30-second control lease per terminal, so two remote views cannot simultaneously type into or resize it. A disconnect releases that client's leases. Another client can acquire an expired lease.

Remote requests have a 1 MiB encoded JSON limit, including protocol overhead. Large environment updates can exceed that limit even when individual values are valid. Such updates are rejected before sending; terminal input accepts the existing 64 KiB frame limit. Reconnect retries use bounded delays. A crashed Tailscale helper requires disabling and re-enabling remote access.

The host must remain running and awake. Reboot, power loss, host application exit, and process crashes do not preserve running shells. Saved session metadata is not a checkpoint of a running process. Reconnection restores a bounded terminal view, not an unlimited permanent transcript.

## Access and security

Tailscale encrypts the network connection and authenticates device identities. Shellfox separately requires host approval before a device can read sessions or control terminals. Revoking a device removes its Shellfox access without terminating the host's terminals. A new device identity requires new approval.

The embedded helper accepts network traffic through Tailscale only. Its local connection to Electron uses per-launch secrets and authenticated peer identity. The renderer cannot make arbitrary network requests or invoke an unrestricted host API. Remote operations exclude application settings, SSH credentials, clipboard access, file-manager integration, updates, and administrator terminal creation.

This is terminal access, not a restricted command sandbox. An approved client can execute commands with the host user's normal permissions, read files that user can read, and access secrets available to that user. Tailnet membership alone does not authorize Shellfox access.

Auth keys travel only to the embedded helper for enrollment. Shellfox does not save them as application settings. Tailscale's durable node identity is stored under the Shellfox user-data directory and must be treated as a credential. Its directory and state file are private to the host user. Windows uses a protected current-user ACL without requiring administrator rights; Unix uses directory mode 0700 and file mode 0600. Deleting or replacing that identity changes the device seen by other hosts.

## Building and verification

The Tailscale helper is a separate Go executable, bundled outside ASAR for Windows, macOS, and Linux. Users of packaged builds do not need Go or an installed VPN application. Development builds require Go 1.27.3 or newer and download the pinned Tailscale module dependencies. Set `SHELLFOX_GO` to an absolute Go executable path if it is not on PATH. `pnpm build` builds and probes the native helper; `pnpm test:tailscale` runs its offline tests and protocol smoke. See `native/tailscale/go.mod` and `scripts/tailscale.mjs` for the build configuration.

Tests with fake transport validate Shellfox authorization, routing, delivery, and disconnect behavior. `tests/e2e/remote-connections.spec.ts` starts two real Electron applications and PTYs, checks host approval, quits the client while a remote command continues, reconnects to the same terminal generation, and revokes access. Only the Tailscale transport is simulated in that test. They do not prove that a real tailnet login, NAT traversal, relay connection, or operating-system package works. Release acceptance must exercise two real devices, including approval, revocation, network loss, client restart, concurrent local access, and host quit. macOS and Linux need native acceptance on those operating systems.

The React Native client in `mobile/` uses the same host approval and terminal protocol. It embeds its own outbound-only Tailscale node, so it needs neither the Tailscale app nor a device-wide VPN. Mobile sessions always run on a remote host. See [Mobile](mobile.md) for builds, Android updates, and device verification.
