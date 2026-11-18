# Linux raw automation

Production RawProcess uses ordinary binary stdin/stdout/stderr pipes, not a PTY. It requires the same existing user delegation, cgroup-v2/cgroup.kill/pidfd and systemd user-scope prerequisites documented in UNIX.md. Darwin remains fail-closed pending a native boundary and real acceptance.

The fixed controller enters the user delegation before the host attaches its original waitable Child and pidfd to a unique pinned cgroup. The authenticated private socket carries GO, the bounded launch specification, START and the guest's actual wait status. Configuration never enters target stdin, argv or the controller environment. The target receives exactly its explicit environment. Public early raw supervisor dispatch must precede normal CLI, logging and GUI startup.

The controller closes its own copies of all three pipe ends after target exec. Only the target and descendants can keep those streams alive. Bytes are preserved without terminal interpretation. Each stream reports real OS EOF after its trailing bytes. The existing shared output queue retains at most1MiB/64 frames; callers must keep draining through both EOFs before finalizing output.

Input uses the existing49152-byte decoded bound, corresponding to65536 canonical base64 wire characters. A whole request is admitted atomically into the bounded queue. Receipt success means every byte was written to the OS pipe, not merely queued. An EOF receipt succeeds only after the actual writer closes. Cancellation during a partial write remains an unconfirmed OS I/O error, never a synthetic zero-byte success or inferred BrokenPipe. Explicit stdin release does not stop the target.

HUP/TERM/INT handlers only set the reviewed controller close flag. The controller uses the original guest pidfd to stop that guest, waits its actual Child status, reports it and kills the held cgroup, including itself and detached descendants. Failed cgroup termination retains the live controller and descriptor for retry. Controller cleanup SIGKILL is never substituted for a guest outcome. Actual guest SIGKILL is137; natural exit137 remains distinguishable. Lost controller status fails explicitly with no invented guest code or finish.

Natural target exit also kills remaining descendants without requiring host polling. Host cleanup requires original-controller wait, kernel populated0 and worker joins. Unconfirmed startup or cleanup returns/retains the actual owner; Drop transfers uncertain resources to a prestarted cleanup thread. Hosts configured to discard child wait status fail before launch rather than claiming original wait proof.

Direct execve uses argv/environment storage prepared before fork. Explicit kernel shebangs work; invalid executable formats do not silently fall back to a shell. No allocations or locks run in pre_exec.

Sealed Linux raw evidence has purpose RawProgram and measured POSIX facts, not Windows CreateProcess/token facts or PTY eligibility. Ordinary raw execution remains Local/Unknown, including when no_new_privs happened to be inherited. It grants no remote input, resize or elevation authority. Windows standard-raw tags and behavior remain unchanged. The Runtime external-source binding selects RawProgram on Linux through its existing private factory; its public binding/evidence APIs are unchanged.
