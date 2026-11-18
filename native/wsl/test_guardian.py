"""Opt-in Linux contract tests. Never launches WSL, a user profile or a distro."""
import base64
import hashlib
import json
import os
from pathlib import Path
import select
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parent
PROJECT = ROOT.parent.parent
sys.dont_write_bytecode = True
OPT_IN = sys.platform == 'linux' and os.environ.get('SHELLFOX_WSL_GUEST_TEST_OPT_IN') == '1'


def send(process, value):
    raw = json.dumps(value).encode()
    process.stdin.write(struct.pack('!I', len(raw)) + raw)
    process.stdin.flush()


def receive(process):
    def exact(size):
        result = bytearray()
        deadline = time.monotonic() + 8
        while len(result) < size:
            left = deadline - time.monotonic()
            if left <= 0 or not select.select([process.stdout], [], [], left)[0]:
                raise TimeoutError('fixture frame timeout')
            part = os.read(process.stdout.fileno(), size - len(result))
            if not part:
                raise EOFError('fixture guardian stopped')
            result.extend(part)
        return bytes(result)
    size = struct.unpack('!I', exact(4))[0]
    if not 0 < size <= 96 * 1024:
        raise ValueError('fixture frame bound')
    return json.loads(exact(size))


def start():
    manifest = [{'name': name, 'path': str(ROOT / filename),
                 'sha256': hashlib.sha256((ROOT / filename).read_bytes()).hexdigest()}
                for name, filename in [('shellfox_wsl_wire', 'wire.py'),
                                       ('shellfox_wsl_tree', 'tree.py'),
                                       ('shellfox_wsl_child', 'child.py'),
                                       ('shellfox_wsl_guardian', 'guardian.py')]]
    return subprocess.Popen([sys.executable, '-I', '-S', '-c',
                             (ROOT / 'bootstrap.py').read_text(), json.dumps(manifest)],
                            stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, bufsize=0)


@unittest.skipUnless(OPT_IN, 'requires Linux and explicit SHELLFOX_WSL_GUEST_TEST_OPT_IN=1')
class GuestContracts(unittest.TestCase):
    def setUp(self):
        scratch = PROJECT / 'tmp'
        scratch.mkdir(exist_ok=True)
        self.scratch = Path(tempfile.mkdtemp(prefix='wsl-g-', dir=scratch))
        self.children = []

    def tearDown(self):
        # Only test-owned subprocess handles, never distro/global PID cleanup.
        for process in self.children:
            if process.poll() is None:
                if process.stdin and not process.stdin.closed:
                    process.stdin.close()
                try:
                    process.wait(timeout=8)
                except subprocess.TimeoutExpired:
                    self.fail('test-owned guardian retained cleanup failure; fixture preserved at ' + str(self.scratch))
            if process.stderr:
                self.assertEqual(process.stderr.read(), b'')
            for stream in (process.stdin, process.stdout, process.stderr):
                if stream and not stream.closed:
                    stream.close()
        shutil.rmtree(self.scratch)

    def process(self):
        process = start()
        self.children.append(process)
        return process

    def launch(self, command):
        process = self.process()
        config = {'mode': 'launch', 'secret': 'b' * 64, 'marker': 'a' * 64,
                  'state': str(self.scratch / 'node'), 'cwd': str(self.scratch),
                  'program': command[0], 'args': command[1:], 'cols': 80, 'rows': 24}
        # Linux CI checkout paths can exceed AF_UNIX length. Keep an explicit
        # caller-selected short path under this project's root tmp directory.
        if len(config['state']) > 80:
            self.skipTest('project scratch path exceeds configured private socket bound')
        send(process, config)
        prepared = receive(process)
        self.assertEqual(prepared['type'], 'prepared')
        send(process, {'action': 'activate'})
        ready = receive(process)
        self.assertEqual(ready['type'], 'ready')
        return process, prepared, ready

    def test_native_preflight_without_a_user_pty(self):
        process = self.process()
        send(process, {'mode': 'probe'})
        proof = receive(process)
        self.assertEqual(proof['protocol'], 1)
        self.assertTrue(proof['pidfd'] and proof['subreaper'] and proof['peercred'])
        self.assertEqual(process.wait(timeout=3), 0)

    def test_raw_pty_and_exact_owned_zero(self):
        process, _, ready = self.launch(['/bin/sh', '-c', "printf '\033[31mraw\033[0m\\n'; sleep 30"])
        identity = ready['status']['identity']
        fd = os.pidfd_open(identity['rootPid'], 0)
        try:
            frame = receive(process)
            self.assertEqual(frame['type'], 'data')
            data = base64.b64decode(frame['data'], validate=True)
            self.assertIn(b'\x1b[31mraw\x1b[0m', data)
            send(process, {'action': 'ack', 'bytes': len(data)})
            send(process, {'action': 'close', 'id': 1})
            while True:
                frame = receive(process)
                if frame['type'] == 'reply':
                    self.assertEqual(frame['status']['identity'], identity)
                    self.assertTrue(frame['status']['cleanupConfirmed'])
                    self.assertEqual(frame['status']['owned'], 0)
                    break
            self.assertTrue(select.select([fd], [], [], 1)[0])
            self.assertEqual(process.wait(timeout=3), 0)
        finally:
            os.close(fd)

    def test_authenticated_recovery_and_eof_cleanup_detached_child(self):
        payload = "import os,time; print('DETACHED',os.getpid(),flush=True); time.sleep(30)"
        code = ("import os,sys,time; p=os.fork();\nif p==0:\n os.setsid(); q=os.fork();\n if q==0:\n"
                "  env=dict(os.environ); env.pop('SHELLFOX_TERMINAL_MARKER',None); "
                "os.execve(sys.executable,[sys.executable,'-I','-S','-c'," + repr(payload) + "],env)\n"
                " os._exit(0)\ntime.sleep(30)")
        process, prepared, ready = self.launch([sys.executable, '-I', '-S', '-c', code])
        captured = bytearray()
        while b'DETACHED ' not in captured or b'\n' not in captured:
            frame = receive(process)
            if frame['type'] == 'data':
                data = base64.b64decode(frame['data'])
                captured.extend(data)
                send(process, {'action': 'ack', 'bytes': len(data)})
        detached = int(bytes(captured).split(b'DETACHED ', 1)[1].split()[0])
        fd = os.pidfd_open(detached, 0)
        recovery = self.process()
        request = {'mode': 'recover', 'endpoint': prepared['endpoint'], 'secret': 'b' * 64,
                   'anchor': prepared['anchor'], 'action': 'status'}
        send(recovery, request)
        self.assertEqual(receive(recovery)['status']['identity'], ready['status']['identity'])
        self.assertEqual(recovery.wait(timeout=3), 0)
        process.stdin.close()
        try:
            self.assertEqual(process.wait(timeout=8), 0)
            self.assertTrue(select.select([fd], [], [], 1)[0])
        finally:
            os.close(fd)

    def test_copied_status_marker_never_adopts_an_unrelated_process(self):
        outsider = subprocess.Popen([sys.executable, '-I', '-S', '-c', 'import time; time.sleep(30)'],
                                    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                    env={**os.environ, 'SHELLFOX_TERMINAL_MARKER': 'a' * 64})
        self.children.append(outsider)
        process, _, _ = self.launch(['/bin/sh', '-c', 'sleep 30'])
        try:
            send(process, {'action': 'close', 'id': 1})
            while True:
                response = receive(process)
                if response['type'] == 'reply':
                    self.assertTrue(response['status']['cleanupConfirmed'])
                    self.assertEqual(response['status']['owned'], 0)
                    break
            self.assertEqual(process.wait(timeout=3), 0)
            self.assertIsNone(outsider.poll())
        finally:
            outsider.terminate()  # Exact test-owned subprocess, not a marker/PID scan.
            outsider.wait(timeout=3)

    def test_wrong_recovery_secret_cannot_close_the_live_session(self):
        process, prepared, _ = self.launch(['/bin/sh', '-c', 'sleep 30'])
        client = self.process()
        send(client, {'mode': 'recover', 'endpoint': prepared['endpoint'], 'secret': 'c' * 64,
                      'anchor': prepared['anchor'], 'action': 'close'})
        self.assertEqual(client.wait(timeout=3), 1)
        self.assertIsNone(process.poll())
        process.stdin.close()
        self.assertEqual(process.wait(timeout=8), 0)

    def test_failed_kill_rolls_back_stops_and_retains_original_pidfds(self):
        code = r'''
import importlib.util, os, signal, subprocess, sys, time
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location('tree', sys.argv[1])
m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
boot = m.preflight(); tree = m.Tree(boot)
child = subprocess.Popen([sys.executable, '-I', '-S', '-c', 'import time; time.sleep(30)'])
tree.pin_child(child.pid, os.getpid())
original = signal.pidfd_send_signal
def deny_kill(fd, sig, info, flags):
    if sig == signal.SIGKILL:
        if sys.argv[2] == 'deadline': time.sleep(5)
        raise PermissionError('injected kill denial')
    return original(fd, sig, info, flags)
signal.pidfd_send_signal = deny_kill
assert not tree.close()
assert tree.live() == 1 and m.row(child.pid)['state'] not in ('T', 't')
signal.pidfd_send_signal = original
assert tree.close(); tree.finish()
'''
        for failure in ('denial', 'deadline'):
            with self.subTest(failure=failure):
                process = subprocess.Popen([sys.executable, '-I', '-S', '-c', code, str(ROOT / 'tree.py'), failure],
                                           stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                self.children.append(process)
                self.assertEqual(process.wait(timeout=8), 0)


if __name__ == '__main__':
    unittest.main()
