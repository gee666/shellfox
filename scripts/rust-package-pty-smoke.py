"""Drive the extracted macOS CLI under a real local tty, without opening a window."""
import errno
import fcntl
import os
import pty
import select
import signal
import struct
import sys
import termios
import time


pid, master = pty.fork()
if pid == 0:
    fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
    os.environ["TERM"] = "xterm-256color"
    os.execv(sys.argv[1], sys.argv[1:])

captured = bytearray()
reaped = False


def drain_once():
    if not select.select([master], [], [], 0.1)[0]:
        return True
    try:
        data = os.read(master, 16384)
    except OSError as error:
        if error.errno != errno.EIO:
            raise
        return False
    if not data:
        return False
    captured.extend(data)
    if len(captured) > 2 * 1024 * 1024:
        raise RuntimeError("packaged CLI PTY output exceeded the fixture limit")
    return True


def wait_for(marker, timeout):
    deadline = time.monotonic() + timeout
    while marker not in captured and time.monotonic() < deadline:
        if not drain_once():
            break
    if marker not in captured:
        raise RuntimeError(f"missing packaged CLI marker {marker!r}; last output: {captured[-2000:]!r}")


try:
    wait_for(b"\x1b[?1049h", 15)
    # The combined marker can only come from the shell, not from echoing input.
    os.write(master, b"printf '%s%s\\n' SHELLFOX_HEADLESS_ ECHO_OK\r")
    wait_for(b"SHELLFOX_HEADLESS_ECHO_OK", 15)
    captured.clear()
    os.write(master, b"\x1d")  # Ctrl-], detach without stopping the host or shell.
    wait_for(b"\x1b[?1049l", 10)
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        child, status = os.waitpid(pid, os.WNOHANG)
        if child == pid:
            reaped = True
            if os.waitstatus_to_exitcode(status) == 0:
                break
            raise RuntimeError(f"packaged CLI attach failed: {captured[-2000:]!r}")
        # The terminal emulator must keep reading through process exit.
        if not drain_once():
            time.sleep(0.05)
    if not reaped:
        raise RuntimeError(f"CLI restored the terminal but did not exit: {captured[-2000:]!r}")
finally:
    os.close(master)
    if not reaped:
        os.kill(pid, signal.SIGKILL)
        os.waitpid(pid, 0)
