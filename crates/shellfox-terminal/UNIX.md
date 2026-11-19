# Unix PTY support

Linux production `PtySession` uses the patched portable-pty allocation and an original waitable controller. It requires:

- cgroup v2 with `cgroup.kill` and pidfd support.
- An existing, writable systemd user delegation at `/sys/fs/cgroup/user.slice/user-<uid>.slice/user@<uid>.service`.
- A running user manager at `/run/user/<uid>/bus` and trusted `/usr/bin/systemd-run` supporting `--expand-environment=no`.
- An absolute trusted executable that calls `shellfox_terminal::supervisor_entry()` before any application initialization.

No elevation, controller configuration, global installation or untracked fallback is attempted. Missing prerequisites fail closed.

## Startup and cleanup

The fixed systemd scope wrapper execs into the same original child PID. This first places an SSH/headless launch inside the existing user delegation. The wrapper does not run the user's command. A private abstract socket and SO_PEERCRED authenticate the waiting controller. The host holds its original Child and pidfd, moves it into a uniquely named pinned cgroup, validates launch facts, then sends GO. Arguments are never expanded by systemd-run.

The controller spawns and waits for the logical command. It assigns foreground terminal control before exec and reports the actual command exit status. Forked, double-forked and setsid descendants remain in the kernel cgroup. Root exit, shutdown or controller loss triggers cgroup-wide termination. Successful host cleanup requires both the original controller's wait result and the kernel's empty populated state. EOF is output ordering, not cleanup proof.

Before READY, the controller installs HUP/TERM/INT handlers that only set an atomic close flag. The event loop routes that flag and host-channel loss through the same held-cgroup termination path. The guest resets its own signals before exec, preserving ordinary Ctrl+C behavior. If cgroup termination fails, the controller stays alive holding the exact descriptor and retries. Host shutdown errors retain the session; Drop transfers remaining resources to a prestarted cleanup thread.

## Privilege policy

`PtyConfig.unix_privilege: UnixPrivilegeMode` is explicit:

- `Local` is the default. It does not set no_new_privs, preserving ordinary local sudo workflows. Its measured POSIX evidence is Unknown and must remain local-only.
- `NonElevating` sets Linux no_new_privs in the original controller before READY. Before GO the host reads the original pinned process's four UID fields, effective/permitted/inheritable/ambient capabilities and no_new_privs. Normal requires the same nonzero host account, all capability sets empty and no_new_privs true. Children inherit this restriction. Setuid/file-capability elevation, including sudo elevation, is unavailable in this mode.

Sealed `NativeLaunchEvidence` uses `OriginalPinnedPosixCredentials` with `PosixLaunchFacts`, not a Windows token. This is source eligibility only. It does not replace authentication, approval, revocation, real leases or other Service admission checks. An existing unrestricted source cannot be upgraded by changing descriptive metadata.

## Environment and platform limits

Linux builds a fresh isolated environment, preserving no inherited hooks, account credentials or askpass carriers. The trusted program overload retains both program leases and sends its bounded six-key askpass payload over the authenticated startup socket. File identities are checked again in the controller before applying the payload only to the target command.

The agreed boundary excludes hostile same-account or administrator interference, including intentional cgroup migration. Ordinary detached descendants do not escape by changing process groups or sessions.

## macOS

The production macOS path opens the same portable-pty master/slave pair. The launched shell starts its own session and group. The host checks the initial executable's set-ID bits and the unreaped shell's real, effective and saved credentials before exposing it. A normal, same-account launch can be shared after approval. Unlike Linux `NonElevating`, this check does not restrict future execs or certify `no_new_privs`.

On close or a `kqueue` exit notification, the host sends HUP and TERM to the original process group, then KILL if needed. It also sweeps same-user processes with the original session ID, including reparented jobs in other groups. A descendant that creates a new session can escape this best-effort cleanup. macOS has no cgroup-equivalent boundary here.

The iOS app is a remote client, not a local PTY host. Windows Job behavior is unchanged.

## Focused tests

On the isolated Linux runner:

```text
cargo check -p shellfox-terminal --tests --features test-support
cargo clippy -p shellfox-terminal --tests --features test-support -- -D warnings
cargo clippy -p shellfox-terminal --lib -- -D warnings
cargo test -p shellfox-terminal --lib --test unix_production --features test-support --no-run
```

Acceptance executes the built artifacts through the owned gated-fork/pidfd runner, not through an untracked shell launch. The actual SSH carrier test runs /usr/bin/ssh against an owned local ProxyCommand without making a network connection. It establishes carrier inheritance, not SSH authentication or Android handoff acceptance.
