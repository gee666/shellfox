"""One guest PTY, persistent pidfd/subreaper owner and recoverable private control."""
import base64
import errno
import fcntl
import hmac
import os
import select
import signal
import socket
import struct
import termios
from shellfox_wsl_wire import Reader, Endpoint, CREDIT, DATA, encode, read_fd, recovery
from shellfox_wsl_tree import Tree, preflight, abort_deadline
from shellfox_wsl_child import validate, dimensions, launch

CLOSING = False


def request_close(_signum, _frame):
    global CLOSING
    CLOSING = True


class Guardian:
    def __init__(self, config, boot):
        self.config = config
        self.tree = Tree(boot)
        self.endpoint = Endpoint(config['state'])
        self.master = None
        self.identity = None
        self.stdout = bytearray()
        self.input = bytearray()
        self.credit = CREDIT
        self.chunk_credit = 32
        self.sequence = 0
        self.reader = Reader()
        self.attached = True
        self.closing = False
        self.master_eof = False
        self.final_report = False
        self.anchor = {'bootId': boot, 'guardianPid': os.getpid(), 'guardianTicks': self.tree.guardian['ticks'], 'marker': config['marker']}

    def status(self):
        return {'identity': self.identity, 'owned': self.tree.live(),
                'cleanupConfirmed': self.tree.confirmed, 'rootExit': self.tree.root_status}

    def emit(self, value):
        if self.attached:
            self.stdout.extend(encode(value))
            if len(self.stdout) > 256 * 1024:
                raise RuntimeError('output queue bound')

    def close(self):
        self.closing = True
        self.input.clear()
        return self.tree.close()

    def command(self, command):
        if not isinstance(command, dict):
            raise ValueError('invalid command')
        action = command.get('action')
        if action == 'ack' and set(command) == {'action', 'bytes'}:
            amount = command['bytes']
            if type(amount) is not int or not 0 < amount <= CREDIT - self.credit or self.chunk_credit >= 32:
                raise ValueError('invalid credit')
            self.credit += amount
            self.chunk_credit += 1
            return
        if action == 'input' and set(command) == {'action', 'data'} and not self.closing:
            data = base64.b64decode(command['data'], validate=True)
            if not data or len(data) > CREDIT or len(self.input) + len(data) > CREDIT:
                raise ValueError('input bound')
            self.input.extend(data)
            return
        if action == 'resize' and set(command) == {'action', 'cols', 'rows'} and not self.closing:
            dimensions(command['cols'], command['rows'])
            fcntl.ioctl(self.master, termios.TIOCSWINSZ, struct.pack('HHHH', command['rows'], command['cols'], 0, 0))
            return
        if action in ('status', 'close') and set(command) == {'action', 'id'} and type(command['id']) is int and 0 < command['id'] < 2**53:
            if action == 'close':
                self.close()
            self.emit({'type': 'reply', 'id': command['id'], 'status': self.status()})
            return
        raise ValueError('invalid control')

    def recover(self):
        stream = self.endpoint.accept()
        try:
            config = read_fd(stream.fileno(), 1)
            if not isinstance(config, dict) or set(config) != {'mode', 'secret', 'anchor', 'endpoint', 'action'} or config['mode'] != 'recover':
                raise ValueError('invalid recovery')
            if not isinstance(config['secret'], str) or config['anchor'] != self.anchor or config['endpoint'] != self.endpoint.path or not hmac.compare_digest(config['secret'], self.config['secret']):
                raise PermissionError('capability mismatch')
            if config['action'] == 'close':
                self.close()
            elif config['action'] != 'status':
                raise ValueError('invalid recovery action')
            response = {'type': 'reply', 'id': 1, 'status': self.status()} if self.identity else {'type': 'aborted', 'anchor': self.anchor, 'cleanup_confirmed': self.tree.confirmed, 'owned': self.tree.live()}
            stream.sendall(encode(response))
        finally:
            stream.close()

    def run(self):
        try:
            os.write(1, encode({'type': 'prepared', 'anchor': self.anchor, 'endpoint': self.endpoint.path}))
            if read_fd(0) != {'action': 'activate'}:
                raise ValueError('activation refused')
            previous_alarm = signal.signal(signal.SIGALRM, abort_deadline)
            signal.setitimer(signal.ITIMER_REAL, 7)
            try:
                self.master, entry = launch(self.config, self.tree, self.endpoint, lambda master: setattr(self, 'master', master))
            finally:
                signal.setitimer(signal.ITIMER_REAL, 0)
                signal.signal(signal.SIGALRM, previous_alarm)
            self.identity = {'bootId': self.tree.boot, 'guardianPid': os.getpid(),
                             'guardianTicks': self.tree.guardian['ticks'], 'rootPid': entry['pid'],
                             'rootTicks': entry['ticks'], 'session': entry['session'], 'marker': self.config['marker']}
            self.emit({'type': 'ready', 'status': self.status(), 'endpoint': self.endpoint.path})
            os.set_blocking(0, False)
            os.set_blocking(1, False)
            while True:
                self.tree.reap()
                if CLOSING or self.tree.root_status is not None:
                    self.closing = True
                if self.closing and not self.tree.confirmed:
                    self.close()
                elif not self.closing:
                    self.tree.track()
                readers = [self.endpoint.socket]
                if self.attached:
                    readers.append(0)
                if self.master is not None and self.credit and self.chunk_credit and not self.master_eof:
                    readers.append(self.master)
                writers = ([1] if self.attached and self.stdout else []) + ([self.master] if self.input and self.master is not None else [])
                readable, writable, _ = select.select(readers, writers, [], .02)
                if self.endpoint.socket in readable:
                    try:
                        self.recover()
                    except Exception:
                        pass
                if 0 in readable:
                    try:
                        data = os.read(0, 32768)
                    except BlockingIOError:
                        data = None
                    if data == b'':
                        self.attached = False
                        self.stdout.clear()
                        self.closing = True
                    elif data:
                        for command in self.reader.push(data):
                            self.command(command)
                if self.master in readable:
                    try:
                        data = os.read(self.master, min(DATA, self.credit))
                    except BlockingIOError:
                        data = None
                    except OSError as error:
                        if error.errno != errno.EIO:
                            raise
                        data = b''
                    if data == b'':
                        self.master_eof = True
                    if data:
                        self.credit -= len(data)
                        self.chunk_credit -= 1
                        self.sequence += 1
                        self.emit({'type': 'data', 'sequence': self.sequence, 'data': base64.b64encode(data).decode('ascii')})
                if self.master in writable:
                    try:
                        size = os.write(self.master, self.input[:DATA])
                    except BlockingIOError:
                        size = 0
                    if size:
                        del self.input[:size]
                        self.emit({'type': 'input_ack', 'bytes': size})
                if 1 in writable:
                    try:
                        size = os.write(1, self.stdout)
                        del self.stdout[:size]
                    except BlockingIOError:
                        pass
                    except BrokenPipeError:
                        self.attached = False
                        self.stdout.clear()
                        self.closing = True
                if self.tree.confirmed and (self.master_eof or not self.attached):
                    if not self.final_report:
                        self.final_report = True
                        self.emit({'type': 'stopped', 'status': self.status()})
                        self.emit({'type': 'eof'})
                    if not self.stdout:
                        return
        except BaseException:
            # EOF/protocol/startup failures cannot abandon a live guest owner.
            was_attached = self.attached
            self.attached = False
            self.closing = True
            while not self.close():
                try:
                    readable, _, _ = select.select([self.endpoint.socket], [], [], .1)
                    if readable:
                        self.recover()
                except Exception:
                    pass
            if was_attached:
                try:
                    if select.select([], [1], [], 1)[1]:
                        os.write(1, encode({'type': 'aborted', 'anchor': self.anchor, 'cleanup_confirmed': True, 'owned': 0}))
                except OSError:
                    pass
        finally:
            if self.tree.confirmed:
                if self.master is not None:
                    os.close(self.master)
                try:
                    self.endpoint.finish()
                finally:
                    self.tree.finish()


def main():
    config = read_fd(0)
    if config.get('mode') == 'recover':
        recovery(config)
        return
    if config == {'mode': 'probe'}:
        boot = preflight()
        left, right = socket.socketpair()
        try:
            _, uid, _ = struct.unpack('=iII', left.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
            if uid != os.geteuid():
                raise RuntimeError('peercred unavailable')
        finally:
            left.close()
            right.close()
        os.write(1, encode({'type': 'capability', 'protocol': 1, 'boot': boot, 'pidfd': True, 'subreaper': True, 'peercred': True}))
        return
    validate(config)
    signal.signal(signal.SIGCHLD, signal.SIG_DFL)
    pid = os.fork()
    if pid:
        # The original WSL invocation is only a waitable tunnel bridge. The
        # persistent owner has a separate guest session and never accepts a
        # Windows PID as guest SO_PEERCRED ancestry evidence.
        for fd in (0, 1, 2):
            os.close(fd)
        while True:
            try:
                _, status = os.waitpid(pid, 0)
                code = os.waitstatus_to_exitcode(status)
                os._exit(code if code >= 0 else 128 - code)
            except InterruptedError:
                pass
    os.setsid()
    for signum in (signal.SIGHUP, signal.SIGTERM, signal.SIGINT):
        signal.signal(signum, request_close)
    boot = preflight()  # Only the persistent owner becomes the launch subreaper.
    Guardian(config, boot).run()
    os._exit(0)
