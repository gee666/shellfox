# Remote connection verification

The desktop remote implementation uses a bundled Go `tsnet` helper. The protocol and UI are shared across Windows, macOS, and Linux; this change does not include an Android application.

## Automated checks

```sh
pnpm typecheck
pnpm test:unit
pnpm test:tailscale
pnpm test:packaging
pnpm build:test
SHELLFOX_VERIFY_PROJECT=e2e pnpm exec playwright test --project=e2e remote-connections.spec.ts
pnpm build
pnpm package
pnpm test:packaged
pnpm test:packaged:startup
pnpm test:packaged:remote
```

Go 1.27.3 or newer must be on PATH, or `SHELLFOX_GO` must name its absolute executable path. All generated state and fake-transport test credentials stay under `tmp/`. The Playwright test compiles its own small Go fixture. That fixture is only selected by the isolated test-build helper override, never by a production build.

The two-application Playwright case uses real Electron main/preload/renderer code, real SQLite session storage, and real PTYs. It checks:

- Unapproved access fails and the host receives a pairing request.
- Approval enables session creation on the host, without creating a client-local session.
- The remote sidebar and terminal view render.
- A remote command continues after the entire client app exits.
- Reopening and reconnecting returns the same terminal generation.
- The host's local window sees that same session.
- Revocation blocks subsequent client operations.

The transport fixture injects signed peer identity in place of the real Tailscale helper. It does not validate WireGuard encryption, actual peer discovery, login, NAT traversal, or relay behavior.

Offline Go tests cover request limits, response limits, routing, identity verification, header replacement, enrollment, and protocol framing. The helper's `--probe` checks the actual compiled binary without starting networking. `pnpm test:packaged:remote` launches the actual packaged app with isolated data, enables its real embedded helper, checks login-required status, and disables it cleanly. It contacts Tailscale without enrolling an account or opening terminals. This is separate from offline unit tests.

## Observed on the Windows x64 development host

- TypeScript typecheck passed.
- Unit suite: 2,177 passed, 50 platform-specific skips.
- Native .NET regression suite: 65 passed.
- Offline Go tests and native helper probe/protocol smoke passed.
- The helper cross-compiled for Windows, macOS, and Linux on x64 and ARM64. Those cross-builds are not native execution tests.
- Storage and real Electron PTY smoke checks passed.
- Packaging configuration tests: 16 passed.
- Six selected Electron e2e cases passed, covering remote reconnect, quit confirmation, IPC/CSP security, directories, and terminal tabs.
- Production Windows build/package, packaged native/backend smoke, renderer startup, and actual embedded-helper sign-in-state bootstrap passed.

The packaged bootstrap exposed an upstream Windows state-directory ACL assumption that the probe alone could not detect. The helper now stores node state atomically and applies a current-user DACL without changing ownership or the primary group. The actual packaged bootstrap was rerun successfully after that fix.

No authenticated two-device tailnet, relay-path, macOS execution, or Linux execution acceptance was performed in this environment.

## Required live-tailnet acceptance

Use two test devices and a dedicated test account or tailnet. Do not use production terminal credentials or unattended destructive commands.

1. Enable remote access on both devices and enroll each through browser login. Repeat enrollment with an untagged auth key on a fresh test identity.
2. Confirm each embedded node appears as a separate Tailscale device. Test the documented access policy for TCP 52743.
3. Connect, deny an unapproved read, approve, create a terminal, and run an interactive CLI application. Confirm it runs on the host OS and uses the host filesystem.
4. Change terminal size, switch tabs, generate output beyond the replay ring, disconnect, and reconnect. Verify the screen and terminal generation rather than merely finding old text.
5. Quit the client completely. Confirm the host command continues, then reopen and reconnect.
6. Interact with the same session in the host's local window. Test two remote clients competing for a control lease and verify local input remains available.
7. Remove network access, restore it, and repeat on a network that requires a Tailscale relay. Verify bounded retries and no duplicate shell creation.
8. Revoke access during an active connection. Later requests must fail. An operation already accepted by the host may finish.
9. Quit the host. Its local terminals must close; the remote client must show disconnection without claiming that the host's processes are still alive.
10. Restart both applications, verify saved device metadata and approvals, and confirm that host restart never claims to restore running shell processes.

Repeat native execution on macOS and Linux and with supported ARM64 builds. Cross-compilation is not native execution acceptance. Packaging must preserve executable permissions, native macOS signing, helper probe output, and third-party notices. The deeper dependency-license audit in `native/tailscale/NOTICE-AUDIT.md` remains a release check.
