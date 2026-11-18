"""Validate isolated launch and create one gated, exact guest PTY child."""
import fcntl
import os
import pty
import select
import signal
import stat
import struct
import termios
from shellfox_wsl_tree import row, exited


def path(value):
    if not isinstance(value, str) or not value.startswith('/') or value.startswith('//') or len(value) > 4096:
        raise ValueError('invalid guest path')
    if any(ord(c) < 32 or ord(c) == 127 for c in value) or any(p in ('.', '..') for p in value.split('/')):
        raise ValueError('invalid guest path')
    return value


def validate(config):
    if config.get('mode') != 'launch' or set(config) != {'mode', 'secret', 'marker', 'state', 'cwd', 'program', 'args', 'cols', 'rows'}:
        raise ValueError('invalid launch')
    for key in ('secret', 'marker'):
        if not isinstance(config[key], str) or len(config[key]) != 64 or any(c not in '0123456789abcdefABCDEF' for c in config[key]):
            raise ValueError('invalid capability')
    if config['secret'].lower() == config['marker'].lower():
        raise ValueError('marker is not an authentication secret')
    for key in ('state', 'cwd', 'program'):
        path(config[key])
    if len(config['state']) > 80 or not isinstance(config['args'], list) or len(config['args']) > 64:
        raise ValueError('launch bound')
    if any(not isinstance(a, str) or len(a) > 8192 or '\0' in a for a in config['args']):
        raise ValueError('argument bound')
    dimensions(config['cols'], config['rows'])
    program = os.path.realpath(config['program'])
    meta = os.stat(program)
    if not stat.S_ISREG(meta.st_mode) or meta.st_uid not in (0, os.geteuid()) or meta.st_mode & 0o022 or not os.access(program, os.X_OK):
        raise ValueError('untrusted guest executable')
    if not os.path.isdir(config['cwd']):
        raise ValueError('guest cwd unavailable')


def dimensions(cols, rows):
    if type(cols) is not int or type(rows) is not int or not 0 < cols <= 32767 or not 0 < rows <= 32767 or cols * rows > 1048576:
        raise ValueError('size bound')


def launch(config, tree, endpoint, remember_master):
    home = config['state'] + '/home'
    os.mkdir(home, 0o700)
    for name in ('.config', '.cache', '.local'):
        os.mkdir(home + '/' + name, 0o700)
    master, slave = pty.openpty()
    remember_master(master)
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', config['rows'], config['cols'], 0, 0))
    ready_r, ready_w = os.pipe2(os.O_CLOEXEC)
    go_r, go_w = os.pipe2(os.O_CLOEXEC)
    error_r, error_w = os.pipe2(os.O_CLOEXEC)
    pid = os.fork()
    if pid == 0:
        try:
            os.close(master)
            for fd in (ready_r, go_w, error_r, endpoint.fd, endpoint.socket.fileno(), tree.guardian_fd):
                os.close(fd)
            os.setsid()
            fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
            for fd in (0, 1, 2):
                os.dup2(slave, fd)
            if slave > 2:
                os.close(slave)
            os.write(ready_w, b'R')
            os.close(ready_w)
            if os.read(go_r, 1) != b'G':
                os._exit(1)
            os.close(go_r)
            for signum in (signal.SIGHUP, signal.SIGTERM, signal.SIGINT, signal.SIGTTOU, signal.SIGTTIN, signal.SIGTSTP):
                signal.signal(signum, signal.SIG_DFL)
            os.chdir(config['cwd'])
            env = {'PATH': '/usr/local/bin:/usr/bin:/bin', 'HOME': home, 'XDG_CONFIG_HOME': home + '/.config',
                   'XDG_CACHE_HOME': home + '/.cache', 'XDG_DATA_HOME': home + '/.local', 'TERM': 'xterm-256color',
                   'COLORTERM': 'truecolor', 'SHELLFOX_TERMINAL_MARKER': config['marker'], 'LANG': 'C.UTF-8'}
            os.execve(config['program'], [config['program']] + config['args'], env)
        except BaseException:
            try:
                os.write(error_w, b'E')
            finally:
                os._exit(1)
    os.close(slave)
    for fd in (ready_w, go_r, error_w):
        os.close(fd)
    # Hold the original direct child before authorizing any user exec.
    tree.pin_child(pid, os.getpid())
    try:
        if not select.select([ready_r], [], [], 3)[0] or os.read(ready_r, 1) != b'R':
            raise RuntimeError('guest setup unconfirmed')
        entry = row(pid)
        if entry['session'] != pid or entry['parent'] != os.getpid():
            raise RuntimeError('guest PTY identity changed')
        tree.held[pid] = (tree.held[pid][0], entry)
        tree.root_identity(pid)
        os.write(go_w, b'G')
        os.close(go_w)
        go_w = -1
        if not select.select([error_r], [], [], 3)[0] or os.read(error_r, 1):
            raise RuntimeError('guest exec unconfirmed')
        if not exited(tree.held[pid][0]):
            try:
                before = row(pid)
                with open('/proc/%d/environ' % pid, 'rb') as stream:
                    env = stream.read(1048577)
                after = row(pid)
                if any(before[key] != entry[key] or after[key] != entry[key] for key in ('ticks', 'parent', 'session')):
                    raise RuntimeError('guest root identity changed')
                if len(env) > 1048576 or ('SHELLFOX_TERMINAL_MARKER=' + config['marker']).encode() not in env.split(b'\0'):
                    raise RuntimeError('guest root marker changed')
            except FileNotFoundError:
                if not exited(tree.held[pid][0]):
                    raise
        os.set_blocking(master, False)
        return master, entry
    finally:
        for fd in (ready_r, error_r, go_w):
            if fd >= 0:
                os.close(fd)


