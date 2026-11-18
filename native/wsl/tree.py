"""Pidfd authority from a dedicated guest subreaper, never marker/PID scans."""
import ctypes
import os
import select
import signal
import time

MAX_OWNED = 256


def abort_deadline(_signum, _frame):
    raise TimeoutError('owned guest operation deadline')


def row(pid):
    with open('/proc/%d/stat' % pid, encoding='ascii') as stream:
        fields = stream.read(16384).rsplit(')', 1)[1].split()
    return {'pid': pid, 'state': fields[0], 'parent': int(fields[1]),
            'session': int(fields[3]), 'ticks': fields[19]}


def children(pid):
    # Only enumerate tasks of a pinned owned process, never all numeric /proc PIDs.
    tasks = os.listdir('/proc/%d/task' % pid)
    if len(tasks) > 2048:
        raise RuntimeError('task bound')
    result = set()
    for task in tasks:
        if not task.isdigit():
            raise RuntimeError('invalid task')
        try:
            with open('/proc/%d/task/%s/children' % (pid, task), encoding='ascii') as stream:
                raw = stream.read(16385)
            if len(raw) > 16384:
                raise RuntimeError('children bound')
            result.update(int(value) for value in raw.split())
        except FileNotFoundError:
            continue
    if len(result) > MAX_OWNED:
        raise RuntimeError('children bound')
    return result


def exited(fd):
    return bool(select.select([fd], [], [], 0)[0])


def preflight():
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.prctl(36, 1, 0, 0, 0) != 0:
        raise RuntimeError('subreaper unavailable')
    value = ctypes.c_int()
    if libc.prctl(37, ctypes.byref(value), 0, 0, 0) != 0 or value.value != 1:
        raise RuntimeError('subreaper unconfirmed')
    fd = os.pidfd_open(os.getpid(), 0)
    try:
        signal.pidfd_send_signal(fd, 0, None, 0)
        if exited(fd):
            raise RuntimeError('self pidfd exited')
        children(os.getpid())
    finally:
        os.close(fd)
    with open('/proc/sys/kernel/random/boot_id', encoding='ascii') as stream:
        return stream.read(128).strip()


class Tree:
    def __init__(self, boot):
        self.boot = boot
        self.guardian = row(os.getpid())
        self.guardian_fd = os.pidfd_open(os.getpid(), 0)
        self.held = {}
        self.root = None
        self.root_ticks = None
        self.root_status = None
        self.root_handle = None
        self.confirmed = False

    def same_guardian(self):
        current = row(os.getpid())
        if exited(self.guardian_fd) or current['ticks'] != self.guardian['ticks']:
            raise RuntimeError('changed guardian')
        with open('/proc/sys/kernel/random/boot_id', encoding='ascii') as stream:
            if stream.read(128).strip() != self.boot:
                raise RuntimeError('changed boot')

    def pin_child(self, pid, parent):
        if pid == os.getpid():
            raise RuntimeError('refuse self signal')
        if pid in self.held:
            if not exited(self.held[pid][0]):
                return False
            os.close(self.held.pop(pid)[0])
        try:
            before = row(pid)
            fd = os.pidfd_open(pid, 0)
        except FileNotFoundError:
            return False
        except ProcessLookupError:
            return False
        try:
            after = row(pid)
            anchor = row(parent)
            if before['ticks'] != after['ticks'] or after['parent'] != parent:
                raise RuntimeError('changed child ancestry')
            if parent == os.getpid():
                if anchor['ticks'] != self.guardian['ticks']:
                    raise RuntimeError('changed guardian ancestry')
            else:
                held = self.held.get(parent)
                if not held or exited(held[0]) or anchor['ticks'] != held[1]['ticks']:
                    raise RuntimeError('lost parent anchor')
            if int(anchor['ticks']) > int(after['ticks']):
                raise RuntimeError('invalid birth order')
            if exited(fd) or after['state'] in ('Z', 'X'):
                os.close(fd)
                return False
            if len(self.held) >= MAX_OWNED:
                raise RuntimeError('owned bound')
            self.held[pid] = (fd, after)
            return True
        except BaseException:
            os.close(fd)
            raise

    def track(self):
        self.same_guardian()
        for pid, (fd, _) in list(self.held.items()):
            if exited(fd):
                os.close(fd)
                del self.held[pid]
        added = False
        parents = [os.getpid()] + [p for p, (fd, _) in self.held.items() if not exited(fd)]
        for parent in parents:
            try:
                offspring = children(parent)
            except FileNotFoundError:
                if parent != os.getpid() and exited(self.held[parent][0]):
                    continue
                raise
            for pid in offspring:
                added = self.pin_child(pid, parent) or added
        return added

    def root_identity(self, pid):
        self.pin_child(pid, os.getpid())
        fd, entry = self.held[pid]
        if exited(fd) or entry['session'] != pid:
            raise RuntimeError('root PTY identity unconfirmed')
        self.root = pid
        self.root_ticks = entry['ticks']
        self.root_handle = os.dup(fd)
        return entry

    def reap(self):
        while True:
            try:
                pid, status = os.waitpid(-1, os.WNOHANG)
            except ChildProcessError:
                return
            if pid == 0:
                return
            if pid == self.root and self.root_status is None:
                self.root_status = os.waitstatus_to_exitcode(status)
                if self.root_status < 0:
                    self.root_status = 128 - self.root_status

    def live(self):
        return sum(not exited(fd) for fd, _ in self.held.values())

    def close(self):
        if self.confirmed:
            return True
        stopped = {}
        deadline = time.monotonic() + 3
        previous_alarm = signal.signal(signal.SIGALRM, abort_deadline)
        signal.setitimer(signal.ITIMER_REAL, 3)
        try:
            stable = 0
            while stable < 2:
                if time.monotonic() > deadline:
                    raise TimeoutError('freeze deadline')
                added = self.track()
                for pid, (fd, identity) in list(self.held.items()):
                    if exited(fd):
                        continue
                    current = row(pid)
                    if current['ticks'] != identity['ticks']:
                        raise RuntimeError('changed pinned birth')
                    if current['state'] not in ('T', 't', 'Z', 'X'):
                        key = (pid, identity['ticks'])
                        if key not in stopped:
                            stopped[key] = os.dup(fd)
                        signal.pidfd_send_signal(fd, signal.SIGSTOP, None, 0)
                all_stopped = all(exited(fd) or row(pid)['state'] in ('T', 't', 'Z', 'X')
                                  for pid, (fd, _) in self.held.items())
                stable = stable + 1 if not added and all_stopped else 0
                time.sleep(.01)
            # Every live process is frozen. Original root is always killed last.
            root = self.root if self.root in self.held and self.held[self.root][1]['ticks'] == self.root_ticks else None
            ordered = [p for p in self.held if p != root]
            def depth(pid):
                seen = set()
                while pid in self.held and pid not in seen:
                    seen.add(pid)
                    pid = self.held[pid][1]['parent']
                return len(seen)
            ordered.sort(key=depth, reverse=True)
            if root is not None:
                ordered.append(root)
            signal.setitimer(signal.ITIMER_REAL, 3)
            for pid in ordered:
                fd = self.held[pid][0]
                if not exited(fd):
                    try:
                        signal.pidfd_send_signal(fd, signal.SIGKILL, None, 0)
                    except ProcessLookupError:
                        pass
            deadline = time.monotonic() + 2
            for fd, _ in self.held.values():
                if not select.select([fd], [], [], max(0, deadline - time.monotonic()))[0]:
                    raise RuntimeError('exit unconfirmed')
            # Subreaper adoption can finish after a root exit. Discover only its
            # own children, and repeat cleanup rather than declaring a false zero.
            self.reap()
            if self.track() or self.live() or children(os.getpid()):
                raise RuntimeError('adoption still pending')
            time.sleep(.02)
            self.reap()
            if self.track() or self.live() or children(os.getpid()):
                raise RuntimeError('zero-owned unconfirmed')
            self.confirmed = True
        except (OSError, RuntimeError, TimeoutError):
            self.confirmed = False
        finally:
            signal.setitimer(signal.ITIMER_REAL, 0)
            signal.signal(signal.SIGALRM, previous_alarm)
            # Roll back every surviving process stopped by this close attempt,
            # including after partial kill dispatch. Keep every original pidfd.
            for fd in stopped.values():
                try:
                    if not exited(fd):
                        signal.pidfd_send_signal(fd, signal.SIGCONT, None, 0)
                except ProcessLookupError:
                    pass
                except OSError:
                    self.confirmed = False
            restore_deadline = time.monotonic() + .5
            for (pid, ticks), fd in stopped.items():
                try:
                    while not exited(fd):
                        current = row(pid)
                        if current['ticks'] != ticks:
                            raise RuntimeError('changed rollback identity')
                        if current['state'] not in ('T', 't'):
                            break
                        if time.monotonic() >= restore_deadline:
                            raise TimeoutError('rollback unconfirmed')
                        time.sleep(.005)
                except (OSError, RuntimeError, TimeoutError):
                    self.confirmed = False
                finally:
                    os.close(fd)
        return self.confirmed

    def finish(self):
        if not self.confirmed:
            raise RuntimeError('refuse unconfirmed owner release')
        for fd, _ in self.held.values():
            os.close(fd)
        if self.root_handle is not None:
            os.close(self.root_handle)
        os.close(self.guardian_fd)
