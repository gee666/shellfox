// This fixture creates only its own PTYs. Cleanup uses held pidfds even when an assertion fails.
export const UNIX_CLOSE_NATIVE_FIXTURE = String.raw`import os, sys, pty, re, select, time, subprocess, json, signal
close_script = sys.argv[1]
boot = open('/proc/sys/kernel/random/boot_id').read().strip()
fixtures = []
def stat(pid): return open('/proc/%d/stat' % pid).read().rsplit(')', 1)[1].split()
def spawn(marker):
    pid, fd = pty.fork()
    if pid == 0:
        os.execve('/bin/bash', ['bash','--noprofile','--norc','-i','-c', "trap '' HUP; sleep 600 & echo CHILD:$!; wait"], dict(os.environ, SHELLFOX_TERMINAL_MARKER=marker))
    output = b''
    deadline = time.monotonic()+5
    while time.monotonic() < deadline:
        if select.select([fd], [], [], .05)[0]: output += os.read(fd,4096)
        m = re.search(rb'CHILD:(\d+)', output)
        if m:
            child = int(m.group(1))
            f = dict(pid=pid,fd=fd,held=os.pidfd_open(pid,0),childfd=os.pidfd_open(child,0),child=child,birth=boot+':'+stat(pid)[19],childBirth=stat(child)[19],marker=marker)
            fixtures.append(f)
            return f
    raise RuntimeError('fixture startup failed')
def close(f, birth=None, known=None):
    return subprocess.run([sys.executable,'-c',close_script,str(f['pid']),birth or f['birth'],f['marker'],json.dumps(known or [])], stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=8)
try:
    owned = spawn('owned-private-marker')
    unrelated = spawn('unrelated-private-marker')
    wrong = close(owned, boot+':'+str(int(owned['birth'].split(':')[-1])+1))
    assert wrong.returncode != 0
    assert not select.select([owned['held'],owned['childfd']],[],[],0)[0]
    assert not select.select([unrelated['held'],unrelated['childfd']],[],[],0)[0]
    result = close(owned)
    if result.returncode: raise RuntimeError(result.stderr.decode())
    assert select.select([owned['held']],[],[],0)[0], 'root survived'
    assert select.select([owned['childfd']],[],[],0)[0], 'HUP-ignoring child survived'
    assert not select.select([unrelated['held'],unrelated['childfd']],[],[],0)[0], 'unrelated process killed'
    orphan = spawn('orphan-private-marker')
    signal.pidfd_send_signal(orphan['held'],signal.SIGKILL,None,0)
    assert select.select([orphan['held']],[],[],3)[0]
    os.waitpid(orphan['pid'],0)
    unknown = close(orphan)
    assert unknown.returncode != 0, 'guessed unproven orphan membership'
    assert not select.select([orphan['childfd']],[],[],0)[0], 'unproven orphan was killed'
    proven = close(orphan, known=[[orphan['child'],orphan['childBirth']]])
    if proven.returncode: raise RuntimeError(proven.stderr.decode())
    assert select.select([orphan['childfd']],[],[],0)[0], 'proven orphan survived'
    assert not select.select([unrelated['held'],unrelated['childfd']],[],[],0)[0]
    print(json.dumps(dict(ok=True, assertions=['owned root and HUP-ignoring background job terminated','wrong birth refused','unrelated PTY remained alive','unproven orphan refused','exact proven orphan terminated'])))
finally:
    for f in fixtures:
        for key in ('childfd','held'):
            if not select.select([f[key]],[],[],0)[0]:
                try: signal.pidfd_send_signal(f[key],signal.SIGKILL,None,0)
                except ProcessLookupError: pass
        try: os.waitpid(f['pid'],0)
        except ChildProcessError: pass
        for key in ('fd','held','childfd'): os.close(f[key])
`;
