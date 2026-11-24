# WSL guest scripts

Scripts that Shellfox runs inside a WSL distribution on Windows. The host embeds them with `include_str!` and runs them through `System32\wsl.exe --exec`, so no guest shell parses a command line built from user text. The scripts WSL tabs use are passed inline and never installed in the distribution.

## Used by WSL tabs

`profile-preflight.sh` decides which distributions appear as profiles (`crates/shellfox-terminal/src/wsl/profile_catalog.rs`). The host lists distributions with `wsl.exe --list --quiet` (UTF-16LE, bounded), then runs the script in each one with `/bin/sh -c`. It checks for `/bin/bash`, `/usr/bin/env` and `python3`, then has Python open a pidfd and signal itself through it, check that the real and effective UIDs match, and read `/proc`. Only the exact `SHELLFOX_PIDFD_READY_V1` reply enables the profile. The check starts no interactive shell and writes nothing in the guest. Profile IDs are opaque `wsl:<distro>` values, never command fragments.

`launch-shell.py` starts a tab's shell (`crates/shellfox-service/src/windows_profiles.rs`). The host first reads the distribution's default account and refuses root. It then runs `wsl.exe --user <name> --exec /usr/bin/python3 -I -S -c <script> <distro> <uid> <name> <cwd> <bash-hook> <zsh-hook>`. The script exits unless the UID, the account name and `WSL_DISTRO_NAME` all match what the host recorded for that tab, changes to the guest folder, and execs the account's login shell. For bash and zsh it first adds the `shellfox` completion hooks other tabs get (`crates/shellfox-terminal/src/completion.rs`). A tab keeps the account it first launched as, even if the distribution's default user changes later.

The tab is an ordinary ConPTY tab. Its Windows Job holds `wsl.exe`. Shellfox doesn't track or clean up Linux processes inside the distribution; what survives a closed tab there is up to WSL.

## Guest guardian (not wired in)

`bootstrap.py`, `wire.py`, `tree.py`, `child.py` and `guardian.py` make up a guest-side guardian that would own a tab's Linux processes. `crates/shellfox-terminal/src/wsl/` embeds them and has the Rust side (`WslGuestGuardian`, `WslSession`), but no Shellfox code starts it. To enable it, the bundle would need to be provisioned, a Python path configured, and the points below tested on a real distribution.

How it works:

- The Windows Job owns `wsl.exe`. The guardian is a separate Python process in the guest. It creates one PTY, becomes a child subreaper, and pins the shell and its descendants with pidfds. Its identity is the boot ID, the guardian and root start ticks, the PTY session and a launch marker.
- Ownership comes from the direct-child relationship and subreaper adoption. The tree walker reads only `/proc/<pinned process>/task/*/children`, never a global PID scan. Inherited or copied markers, including those seen in nested WSL, never make a process eligible for cleanup. Detached and double-forked children stay owned through adoption and their original pidfds.
- Close freezes the owned tree to a fixed point, confirms the freezes, kills descendants before the root through pidfds, and checks that every held process exited. It reaps adopted children and checks twice that none remain. Separate timers bound freezing, termination and startup. A failed freeze or partial kill resumes every process that attempt stopped. On failure the guardian, its endpoint and the pidfds stay for a retry. There is no `wsl --terminate`, distro kill, kill by name, global PID scan or numeric-PID fallback.
- Startup needs an explicit consent object, an absolute trusted guest Python and a bundle directory. The command is `wsl.exe --distribution <distro> --exec <python> -I -S -c <bootstrap> <manifest>`. The bootstrap checks isolated, no-site mode, the interpreter's ownership and each bundled file's SHA-256 before running it. Bundle filenames are fixed and Python imports no user packages.
- `WslGuestGuardian::preflight` checks pidfds, `pidfd_send_signal`, subreaping, procfs access to owned children and guest `SO_PEERCRED`. A cached profile or status marker can't stand in for it. The spawn API refuses non-Windows hosts.
- The guardian creates a private state directory and endpoint, then sends `Prepared` with its identity. The host puts `wsl.exe` in its Job and records recovery data before sending `activate`. Only then does the guardian open the PTY and start the shell, and it confirms the exec and root environment before `Ready`. Nothing assumes a Windows parent appears as the Linux socket's peer PID.
- Stdio frames are four-byte big-endian length-prefixed JSON, at most 96 KiB. Output is base64 in ordered frames of at most 4096 raw bytes, with a 64 KiB byte window and a 32-frame window. Input is one bounded base64 frame per write, with a 64 KiB window, acknowledged only after the PTY write. Python does no terminal emulation; Ghostty on the host is the only parser.
- The private AF_UNIX endpoint accepts only `status` and `close`. Recovery uses a random 32-byte secret and the guardian's boot, start-tick and marker identity, compared in constant time. Both sides check `SO_PEERCRED`, and the reconnecting client also checks the guardian's PID and start ticks. `RecoveryEvidence` is a credential: don't log it, show it or store it in general preferences. Restoring a saved record doesn't prove the guardian is alive; STATUS or CLOSE must authenticate it. Storing the record across host restarts is the caller's job.
- EOF, bad framing, root exit and termination signals start cleanup. If cleanup fails, the guardian and endpoint stay up. `WslSession::shutdown` keeps its handles on failure, and Drop hands them to a prestarted retry thread instead of dropping the Windows Job.
- `WslUploadMapping` maps one approved Windows staging root to one approved guest root for one distribution. It doesn't guess `/mnt/<drive>`, run `wslpath` or authorize uploads. Files must stay inside the staging root and can't be symlinks, reparse points or hard links.

## Tests

The Rust tests in `crates/shellfox-terminal` use a fake `wsl.exe` fixture. They cover UTF-16 discovery, argument separation, the preflight, `Prepared` and activation sequence, byte transport, close confirmation, failed-close retention and retry. They don't exercise a real WSL kernel or distribution.

`test_guardian.py` tests the guardian on Linux with processes it starts itself and scratch space under the project's `tmp/`:

```sh
SHELLFOX_WSL_GUEST_TEST_OPT_IN=1 python3 -I -S native/wsl/test_guardian.py
```

It covers the pidfd and subreaper preflight, a real PTY, raw bytes, zero remaining processes after close, detached descendants, EOF cleanup, authenticated recovery, a wrong secret, copied markers and freeze rollback after a refused kill and after a timeout. Run it on a disposable Linux machine or CI runner, not inside a WSL distribution you use.
