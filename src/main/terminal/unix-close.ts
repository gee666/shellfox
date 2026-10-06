import { readFile, open } from 'node:fs/promises';
import type { TrackingRoot } from './tracking';
import { execute, type Execute } from './profiles';

export const PIDFD_READY = 'SHELLFOX_PIDFD_READY_V1';
export const PIDFD_PREFLIGHT = `import os, signal, select
assert callable(getattr(os, 'pidfd_open', None)) and callable(getattr(signal, 'pidfd_send_signal', None))
assert os.getuid() == os.geteuid()
open('/proc/sys/kernel/random/boot_id').read()
open('/proc/self/stat').read()
fd = os.pidfd_open(os.getpid(), 0)
try: signal.pidfd_send_signal(fd, 0, None, 0)
finally: os.close(fd)
print('${PIDFD_READY}')
`;
export interface UnixIdentity { pid: number; birth: string }
/** Freeze first, enumerate to a fixed point, then signal held pidfds, not recycled numeric PIDs.
 * Whole PTY sessions are admitted only while an exact owned root/member anchors the session.
 * Previously proven detached members are admitted by exact birth, not by marker alone. */
export const UNIX_TREE_CLOSE_SCRIPT = `import os, sys, signal, select, time, json
pid, expected, marker = int(sys.argv[1]), sys.argv[2], sys.argv[3]
boot, ticks = expected.rsplit(':', 1)
assert open('/proc/sys/kernel/random/boot_id').read().strip() == boot
known = json.loads(sys.argv[4])
assert len(known) <= 256
held, stopped = {}, set()
committed = False
def abort(signum, frame): raise TimeoutError('owned close helper interrupted')
signal.signal(signal.SIGTERM, abort)
signal.signal(signal.SIGALRM, abort)
# A self deadline runs before the outer command timeout, so pre-commit stops are rolled back.
signal.setitimer(signal.ITIMER_REAL, 3)
def row(p):
    s = open('/proc/%d/stat' % p).read().rsplit(')', 1)[1].split()
    return dict(pid=p, state=s[0], parent=int(s[1]), sid=int(s[3]), ticks=s[19])
def ready(fd): return bool(select.select([fd], [], [], 0)[0])
def pin(p, birth, sid=None):
    if p == os.getpid(): raise RuntimeError('refuse helper self signal')
    try: fd = os.pidfd_open(p, 0)
    except ProcessLookupError: return None
    try:
        r = row(p)
        if r['ticks'] != birth: raise RuntimeError('changed birth')
        if sid is not None and r['sid'] != sid: raise RuntimeError('changed session')
        if ready(fd) or r['state'] in ('Z', 'X'): os.close(fd); return None
        held[p] = (fd, r)
        return r
    except FileNotFoundError: os.close(fd); return None
    except BaseException: os.close(fd); raise
def freeze(p):
    fd, r = held[p]
    if ready(fd): return
    if r['state'] not in ('T', 't'):
        try: signal.pidfd_send_signal(fd, signal.SIGSTOP, None, 0); stopped.add(p)
        except ProcessLookupError: return
def snapshot():
    rows = {}
    for name in os.listdir('/proc'):
        if not name.isdigit(): continue
        try: rows[int(name)] = row(int(name))
        except FileNotFoundError: pass
        if len(rows) > 20000: raise RuntimeError('snapshot bound')
    return rows
try:
    root = pin(pid, ticks)
    if root:
        if root['sid'] != pid: raise RuntimeError('shell does not own an isolated PTY session')
        env = open('/proc/%d/environ' % pid, 'rb').read(1048576).split(b'\\0')
        if ('SHELLFOX_TERMINAL_MARKER=' + marker).encode() not in env: raise RuntimeError('changed root marker')
        freeze(pid)
    # Old detached descendants still need an exact held identity. A mismatched cache is ignored.
    for k in known:
        p, birth = int(k[0]), str(k[1])
        if p in held: continue
        try: pin(p, birth)
        except RuntimeError: continue
    # Cached members may change sessions. Only the original live session leader is an immutable SID anchor.
    def root_anchor():
        if pid not in held or ready(held[pid][0]): return False
        r = row(pid)
        if r['ticks'] != ticks or r['sid'] != pid: raise RuntimeError('changed root session identity')
        if r['state'] not in ('T','t','Z','X'): return False
        return r['state'] in ('T','t')
    for p in list(held): freeze(p)
    for _ in range(40):
        if root is None or root_anchor() or ready(held[pid][0]): break
        time.sleep(.005)
    stable = 0
    for attempt in range(40):
        rows = snapshot()
        owned = {p for p, (fd, r) in held.items() if not ready(fd) and p in rows and rows[p]['ticks'] == r['ticks']}
        anchored_now = root_anchor()
        if anchored_now: owned.update(p for p, r in rows.items() if r['sid'] == pid)
        elif any(r['sid'] == pid and r['state'] not in ('Z','X') and p not in owned for p, r in rows.items()):
            raise RuntimeError('owned session anchor was lost; refusing unproven members')
        # This also includes children of already proven detached processes.
        for _ in range(64):
            expanded = owned | {p for p, r in rows.items() if r['parent'] in owned and r['parent'] in rows and int(rows[r['parent']]['ticks']) <= int(r['ticks'])}
            if expanded == owned: break
            owned = expanded
        if len(owned) > 256: raise RuntimeError('owned tree bound')
        added = False
        for p in owned:
            if p in held or p not in rows or rows[p]['state'] in ('Z','X'): continue
            r = rows[p]
            if pin(p, r['ticks'], r['sid']): freeze(p); added = True
        stable = 0 if added else stable + 1
        if stable >= 2: break
        time.sleep(.01)
    else: raise RuntimeError('tree did not stabilize')
    # Every live held member has stopped before any kill is committed.
    for p, (fd, r) in held.items():
        if ready(fd): continue
        current = row(p)
        if current['ticks'] != r['ticks'] or current['state'] not in ('T','t','Z','X'):
            raise RuntimeError('freeze not confirmed')
    # Freeze and termination have separate self deadlines. Commit is not proof that a signal succeeded.
    signal.setitimer(signal.ITIMER_REAL, 3)
    committed = True
    for p, (fd, r) in held.items():
        if p == pid or ready(fd): continue
        try: signal.pidfd_send_signal(fd, signal.SIGKILL, None, 0)
        except ProcessLookupError: pass
    if pid in held and not ready(held[pid][0]):
        try: signal.pidfd_send_signal(held[pid][0], signal.SIGKILL, None, 0)
        except ProcessLookupError: pass
    deadline = time.monotonic() + 2
    for fd, r in held.values():
        if not select.select([fd], [], [], max(0, deadline-time.monotonic()))[0]:
            raise RuntimeError('owned descendant exit not confirmed')
    # Do not use a dead leader's recyclable SID to infer new ownership after all held members exited.
finally:
    signal.setitimer(signal.ITIMER_REAL, 0)
    # Any surviving process stopped by this invocation is restored, even after partial kill dispatch.
    # A held pidfd can never resume a reused PID. One restore failure must not suppress the others.
    restore_errors = []
    for p in stopped:
        fd = held[p][0]
        if ready(fd): continue
        try: signal.pidfd_send_signal(fd, signal.SIGCONT, None, 0)
        except ProcessLookupError: pass
        except BaseException as e: restore_errors.append(str(e))
    for fd, r in held.values(): os.close(fd)
    if restore_errors: raise RuntimeError('owned survivors could not all be restored: '+','.join(restore_errors))
`;
export function compactOwnedIdentities(identity: UnixIdentity, known: UnixIdentity[]): [number, string][] {
  const boot = identity.birth.split(':')[0];
  const proven = [...new Map(known.filter(p => Number.isSafeInteger(p.pid) && p.pid > 0 && p.birth.startsWith(boot + ':') && /^\d+$/.test(p.birth.slice(boot.length + 1))).map(p => [`${p.pid}:${p.birth}`, p])).values()];
  if (proven.length > 256) throw new Error('The proven descendant count exceeds the safe close bound.');
  return proven.map(p => [p.pid, p.birth.slice(boot.length + 1)]);
}
export async function closeUnixTree(root: TrackingRoot, identity: UnixIdentity | null, known: UnixIdentity[] = [], run: Execute = execute): Promise<void> {
  if (process.platform !== 'linux' || !identity || !/^[0-9a-fA-F-]{32,36}:\d+$/.test(identity.birth)) throw new Error('Safe Unix process-tree termination is unavailable.');
  await run('/usr/bin/python3', ['-c', UNIX_TREE_CLOSE_SCRIPT, String(identity.pid), identity.birth, root.marker, JSON.stringify(compactOwnedIdentities(identity, known))]);
}
/** Capture before accepting input. Marker, parent and stat are rechecked around the read. */
export async function captureLinuxRoot(root: TrackingRoot): Promise<UnixIdentity | null> {
  if (process.platform !== 'linux' || root.environment !== 'local') return null;
  try {
    const parse = (text: string) => { const fields = text.slice(text.lastIndexOf(')') + 2).split(/\s+/); return { parent: Number(fields[1]), sid: Number(fields[3]), ticks: fields[19] }; };
    const before = parse(await readFile(`/proc/${root.pid}/stat`, 'utf8'));
    const fd = await open(`/proc/${root.pid}/environ`, 'r'); let env: string;
    try { const buffer = Buffer.alloc(1024 * 1024); const { bytesRead } = await fd.read(buffer, 0, buffer.length, 0); env = buffer.toString('utf8', 0, bytesRead); } finally { await fd.close(); }
    const after = parse(await readFile(`/proc/${root.pid}/stat`, 'utf8'));
    if (before.parent !== process.pid || before.sid !== root.pid || before.ticks !== after.ticks || before.parent !== after.parent || before.sid !== after.sid || !env.split('\0').includes(`SHELLFOX_TERMINAL_MARKER=${root.marker}`)) return null;
    const boot = (await readFile('/proc/sys/kernel/random/boot_id', 'utf8')).trim();
    return { pid: root.pid, birth: `${boot}:${before.ticks}` };
  } catch { return null; }
}
