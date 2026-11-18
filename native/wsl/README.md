# Retained WSL guest guardian

This is source for an opt-in WSL terminal boundary, not permission to enable WSL profiles automatically. No real distro, guest interpreter, shell, profile or account was used to develop the Windows fixtures. Actual WSL runtime acceptance remains blocked until an explicitly approved isolated guest test is available.

## Ownership

The Windows Job owns the original Windows `wsl.exe` process handles. It does not own Linux processes. The guest guardian is a separate persistent Python process. It creates exactly one guest PTY, becomes a Linux child subreaper and pins its original shell and proven descendants with pidfds. Boot ID, guardian/root start ticks, isolated PTY session and launch marker form the reported identity.

Ownership comes from the original direct-child relationship and kernel subreaper adoption. The tree walker reads only `/proc/<pinned process>/task/*/children`, never a global numeric PID scan. The marker confirms the launched root environment. Inherited or copied markers, including nested WSL status observations, never admit a process for cleanup. Detached and double-forked children remain owned through subreaper adoption and original pidfds.

Close follows the active `src/main/terminal/unix-close.ts` freeze/pidfd/rollback sequence, but replaces global enumeration with guardian-scoped child ownership. It freezes the owned tree to a fixed point, confirms the freezes, kills descendants before the original root through pidfds, and verifies every held process exited. Separate self timers bound freeze and termination; startup has its own timer before input is enabled. It also reaps adopted children and verifies the guardian has no remaining children twice. Any failed freeze or partial dispatch restores all surviving processes stopped by that attempt. Failure retains the guardian, its private endpoint and original pidfds for retry. There is no `wsl --terminate`, distro kill, process-name kill, global PID scan or numeric-PID fallback.

## Startup and transport

The host discovers only the absolute OS `System32/wsl.exe`. Distro enumeration is bounded UTF-16LE `--list --quiet`; it never invokes a guest. Profile IDs are opaque `wsl:<distro>` values, not shell command fragments.

Starting a guest requires an explicit consent object, configured absolute trusted guest Python and a provisioned bundle directory. The command is `wsl.exe --distribution <distro> --exec <python> -I -S -c <fixed bootstrap> <artifact manifest>`. No shell evaluates the command. The bootstrap verifies isolated/no-site mode, trusted interpreter ownership and every bundled source SHA-256 before executing it. Bundle filenames are fixed. Python startup imports no user packages or profiles.

`WslGuestGuardian::preflight` performs actual guest checks for pidfds, `pidfd_send_signal`, subreaping, owned-child procfs access and guest `SO_PEERCRED`. A native capability cannot be reconstructed from cached profiles or status markers. The production spawn API rejects non-Windows hosts. Test-support injection is separate and must never be used to advertise production availability.

The guardian first creates a new private state directory and endpoint, then sends `Prepared` with its exact guardian identity. The host owns the Windows transport Job and records recovery evidence before sending `activate`. Only then may the guardian allocate a PTY and release the shell. It confirms exec and the root environment before `Ready`. No assumption is made that a Windows parent can appear as the Linux socket's peer PID.

Stdio uses four-byte big-endian length-prefixed JSON. Frames are capped at 96 KiB. Terminal output is base64 in ordered frames of at most 4096 raw bytes, with both a 64 KiB byte window and a 32-frame window. Input is one bounded base64 frame per write, with a 64 KiB aggregate window and acknowledgements only after PTY writes. No terminal emulation runs in Python. Host Ghostty remains the only authoritative parser. Diagnostics are discarded in fixed scratch storage and never become user-facing errors.

## Recovery

The private AF_UNIX endpoint accepts only `status` and `close`, never input, resize or caller-supplied process identities. The recovery capability includes an unpredictable 32-byte secret and exact guardian boot/start-tick/marker anchor. Both peers check actual guest `SO_PEERCRED`; the reconnecting guest client also verifies the endpoint's exact guardian PID and start ticks. The server compares the capability in constant time. A Windows parent PID is never used as guest socket authentication.

EOF, invalid framing, root exit and termination signals initiate owned cleanup. A failed cleanup keeps the guardian alive and the endpoint usable. `WslSession::shutdown` retains its owners on failure. Drop transfers them to a prestarted retry owner, rather than dropping the Windows Job as pretend guest cleanup. `RecoveryEvidence` is a credential; do not log it, send it to a renderer or store it in general preferences. The Rust API supports recovery after tunnel failure within the retained session. `RecoveryEvidence::restore` also reconstructs a bounded record from the caller's private credential storage. Restoring a record is not liveness proof; STATUS/CLOSE must authenticate the current guardian. Secure credential publication and storage across host restarts remain the caller's responsibility.

## Upload paths

`WslUploadMapping` maps a separately approved native staging root to a separately approved guest root for one distro. It does not guess `/mnt/<drive>`, execute arbitrary `wslpath` commands or grant upload authorization. The files adapter must revalidate approval, generation and terminal control before delegating. Native files must remain inside the staging root and cannot be symlink, reparse or hardlink aliases. The caller must establish guest mount visibility before approving the mapping.

## Verification

Rust fake-wsl tests execute an owned fixture binary only. They cover UTF-16 discovery, fixed argument separation, the native-proof/Prepared/activation sequence, exact byte transport, close proof, failed-close retention and retry. They are not evidence that an installed WSL kernel or distro works.

Linux contract tests exist in `test_guardian.py`. They launch only their own isolated fixture processes and use project-root `tmp/` scratch. On a Linux CI runner, with explicit test permission:

```sh
SHELLFOX_WSL_GUEST_TEST_OPT_IN=1 python3 -I -S native/wsl/test_guardian.py
```

They cover real pidfd/subreaper preflight, a real guest-style PTY, raw bytes, exact zero-owned confirmation, detached descendants, EOF cleanup, authenticated recovery, wrong-secret denial, copied-marker nonauthority and freeze rollback after both kill denial and a self deadline. Do not run that command inside a user's real WSL distro without separate authorization. No automatic distro import, registration, termination, elevation or UAC operation belongs in these tests.

The guest bundle consists of `wire.py`, `tree.py`, `child.py` and `guardian.py`. The host embeds `bootstrap.py` and the expected source digests.

Before enabling a production profile, provision the byte-identical reviewed bundle through an approved installer, configure and verify the Python path, then obtain native WSL acceptance for exec failure, network/host interruption, child churn, detached descendants, copied markers, recovery and Windows Job lifetime. Source implementation and fake tests alone do not close that gate.
