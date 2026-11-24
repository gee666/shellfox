# Unix PTY support

Linux production `PtySession` uses the patched portable-pty allocation and an original waitable controller, launched by an absolute trusted executable that calls `shellfox_terminal::supervisor_entry()` before any application initialization. Each process picks its containment once, on first use, and logs it (`Terminal containment: cgroup|session (reason)`):

- **cgroup**, preferred: a writable cgroup-v2 systemd user delegation at `/sys/fs/cgroup/user.slice/user-<uid>.slice/user@<uid>.service` with `cgroup.kill`, and a trusted `/usr/bin/systemd-run` that passes a `--user --scope` probe. It kills the whole tree at once and keeps an OOM kill inside one terminal.
- **session**, otherwise: root without a user manager, containers, WSL or distros without systemd, cgroup v1, SSH logins without a user manager. The controller runs directly, leads its own session and is the subreaper for its descendants.

Both need pidfd support. The real and effective user IDs must match; root is allowed. Shells get `oom_score_adj` 300 in both modes.

## Startup and cleanup

In cgroup mode the fixed systemd scope wrapper execs into the same original child PID. This first places an SSH/headless launch inside the existing user delegation. The wrapper does not run the user's command. A private abstract socket and SO_PEERCRED authenticate the waiting controller. The host holds its original Child and pidfd, moves it into a uniquely named pinned cgroup (or, in session mode, checks that it leads its session), validates launch facts, then sends GO. Arguments are never expanded by systemd-run.

The controller spawns and waits for the logical command. It assigns foreground terminal control before exec and reports the actual command exit status. In cgroup mode forked, double-forked and setsid descendants remain in the kernel cgroup, and root exit, shutdown or controller loss triggers cgroup-wide termination. In session mode the controller SIGKILLs every process in its session and every orphan reparented to it, each pinned by pidfd and re-checked through its /proc directory, until none is left, then kills itself. The host does the same sweep as a backstop and SIGKILLs a controller that hasn't finished within 2 s. A process that starts its own session while its parent is alive, and is still running after the controller dies, escapes; there is no kernel boundary. So does a setuid member (sudo, su) of a non-root Local tab, which this account may not signal; a remote tab runs with no_new_privs and has none. The sweep fails closed when /proc shows another PID namespace. The mode is chosen once per process: a host started before its user manager stays in session mode until restarted. Successful host cleanup requires the controller's exit and an empty cgroup or session; the controller is reaped only then, so its PID keeps naming the session. EOF is output ordering, not cleanup proof.

Before READY, the controller installs HUP/TERM/INT handlers that only set an atomic close flag. The event loop routes that flag and host-channel loss through the same teardown: the held cgroup kill, or the session sweep. The guest resets its own signals before exec, preserving ordinary Ctrl+C behavior. If termination fails, the controller stays alive and retries. Host shutdown errors retain the session; Drop transfers remaining resources to a prestarted cleanup thread.

## Privilege policy

`PtyConfig.unix_privilege: UnixPrivilegeMode` is explicit:

- `Local` is the default. It does not set no_new_privs, preserving ordinary local sudo workflows. Its measured POSIX evidence is Unknown and must remain local-only.
- `NonElevating` sets Linux no_new_privs in the original controller before READY. Before GO the host reads the original pinned process's four UID fields, effective/permitted/inheritable/ambient capabilities and no_new_privs. Normal requires the same host account and no_new_privs true. A non-root account also needs all capability sets empty; root keeps its own. A root-run host is a normal host, as in 0.9, so remote devices get its root shells. Children inherit this restriction. Setuid/file-capability elevation, including sudo elevation, is unavailable in this mode.

Sealed `NativeLaunchEvidence` uses `OriginalPinnedPosixCredentials` with `PosixLaunchFacts`, not a Windows token. This is source eligibility only. It does not replace authentication, approval, revocation, real leases or other Service admission checks. An existing unrestricted source cannot be upgraded by changing descriptive metadata.

## Environment and platform limits

Linux builds a fresh isolated environment, preserving no inherited hooks, account credentials or askpass carriers. The trusted program overload retains both program leases and sends its bounded six-key askpass payload over the authenticated startup socket. File identities are checked again in the controller before applying the payload only to the target command.

The agreed boundary excludes hostile same-account or administrator interference, including intentional cgroup migration. In cgroup mode ordinary detached descendants do not escape by changing process groups or sessions; in session mode see the limit above.

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
