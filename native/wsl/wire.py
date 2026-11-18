"""Bounded private framing. Terminal bytes never share a parser with controls."""
import json
import os
import select
import socket
import stat
import struct
import time

LIMIT = 96 * 1024
DATA = 4096
CREDIT = 64 * 1024


def unique(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('duplicate field')
        result[key] = value
    return result


def decode(data):
    return json.loads(data, object_pairs_hook=unique,
                      parse_constant=lambda _: (_ for _ in ()).throw(ValueError('nonfinite')))


def encode(value):
    data = json.dumps(value, separators=(',', ':'), ensure_ascii=True).encode()
    if not data or len(data) > LIMIT:
        raise ValueError('frame bound')
    return struct.pack('!I', len(data)) + data


class Reader:
    def __init__(self):
        self.buffer = bytearray()

    def push(self, data):
        self.buffer.extend(data)
        if len(self.buffer) > 2 * LIMIT + 8:
            raise ValueError('buffer bound')
        frames = []
        while len(self.buffer) >= 4:
            size = struct.unpack('!I', self.buffer[:4])[0]
            if not 0 < size <= LIMIT:
                raise ValueError('frame bound')
            if len(self.buffer) < size + 4:
                break
            frames.append(decode(bytes(self.buffer[4:4 + size])))
            del self.buffer[:4 + size]
        return frames


def read_fd(fd, timeout=8):
    deadline = time.monotonic() + timeout
    data = bytearray()
    size = 4
    while len(data) < size:
        left = deadline - time.monotonic()
        if left <= 0 or not select.select([fd], [], [], left)[0]:
            raise TimeoutError('frame deadline')
        part = os.read(fd, size - len(data))
        if not part:
            raise EOFError('frame eof')
        data.extend(part)
        if len(data) == 4 and size == 4:
            length = struct.unpack('!I', data)[0]
            if not 0 < length <= LIMIT:
                raise ValueError('frame bound')
            size += length
    return decode(bytes(data[4:]))


class Endpoint:
    def __init__(self, directory):
        os.mkdir(directory, 0o700)
        self.directory = directory
        self.fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
        meta = os.fstat(self.fd)
        if meta.st_uid != os.geteuid() or meta.st_mode & 0o077:
            raise ValueError('unsafe state directory')
        self.path = directory + '/control.sock'
        self.socket = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.socket.set_inheritable(False)
        self.socket.bind(self.path)
        os.chmod(self.path, 0o600)
        self.inode = os.stat('control.sock', dir_fd=self.fd, follow_symlinks=False).st_ino
        self.socket.listen(4)
        self.socket.setblocking(False)

    def accept(self):
        stream, _ = self.socket.accept()
        stream.settimeout(1)
        credentials = stream.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12)
        _, uid, _ = struct.unpack('=iII', credentials)
        if uid != os.geteuid():
            stream.close()
            raise PermissionError('guest peer uid')
        return stream

    def finish(self):
        self.socket.close()
        try:
            info = os.stat('control.sock', dir_fd=self.fd, follow_symlinks=False)
            if stat.S_ISSOCK(info.st_mode) and info.st_ino == self.inode:
                os.unlink('control.sock', dir_fd=self.fd)
        finally:
            os.close(self.fd)
        # Keep isolated home/state for caller-owned lifecycle. Never recursive-delete.


def recovery(config):
    if set(config) != {'mode', 'endpoint', 'secret', 'anchor', 'action'}:
        raise ValueError('invalid recovery')
    if config['action'] not in ('status', 'close'):
        raise ValueError('invalid action')
    stream = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    stream.settimeout(8)
    stream.connect(config['endpoint'])
    pid, uid, _ = struct.unpack('=iII', stream.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
    from shellfox_wsl_tree import row
    with open('/proc/sys/kernel/random/boot_id', encoding='ascii') as boot:
        current_boot = boot.read(128).strip()
    anchor = config['anchor']
    if uid != os.geteuid() or pid != anchor['guardianPid'] or row(pid)['ticks'] != anchor['guardianTicks'] or current_boot != anchor['bootId']:
        raise PermissionError('guest endpoint identity')
    stream.sendall(encode(config))
    response = read_fd(stream.fileno())
    os.write(1, encode(response))
    stream.close()
