// Isolated PID namespace only. Held pidfds clean up every fixture-owned child.
export const UNIX_SAFETY_NATIVE_FIXTURE = String.raw`import os,sys,pty,signal,select,time,subprocess,json,re
original=sys.argv[1]
boot=open('/proc/sys/kernel/random/boot_id').read().strip()
held=[]; children=[]; masters=[]
def hold(pid):
    fd=os.pidfd_open(pid,0); held.append(fd); return fd
def ready(fd): return bool(select.select([fd],[],[],0)[0])
def row(pid):
    s=open('/proc/%d/stat'%pid).read().rsplit(')',1)[1].split()
    return dict(state=s[0],sid=int(s[3]),ticks=s[19])
def signal_fd(fd,s):
    if not ready(fd): signal.pidfd_send_signal(fd,s,None,0)
def wait_for(test,seconds=3):
    end=time.monotonic()+seconds
    while time.monotonic()<end:
        if test(): return
        time.sleep(.005)
    raise RuntimeError('phase timed out')
def spawn_tree(marker,fixed_pid=None):
    if fixed_pid is not None: open('/proc/sys/kernel/ns_last_pid','w').write(str(fixed_pid-1))
    pid,master=pty.fork()
    if pid==0:
        code="""import os,signal,time
signal.signal(signal.SIGHUP,signal.SIG_IGN)
c=os.fork()
if c==0:
    def detach(sig,frame): os.setsid()
    signal.signal(signal.SIGUSR1,detach)
    while True: signal.pause()
else:
    print('CHILD:%d'%c,flush=True)
    while True: signal.pause()
"""
        os.execve(sys.executable,[sys.executable,'-c',code],dict(os.environ,SHELLFOX_TERMINAL_MARKER=marker))
    children.append(pid); masters.append(master); rootfd=hold(pid)
    if fixed_pid is not None and pid!=fixed_pid: raise RuntimeError('isolated PID allocation did not match')
    data=b''; end=time.monotonic()+3
    while time.monotonic()<end:
        if select.select([master],[],[],.05)[0]: data+=os.read(master,4096)
        m=re.search(rb'CHILD:(\d+)',data)
        if m:
            child=int(m.group(1)); childfd=hold(child); children.append(child)
            return dict(pid=pid,fd=rootfd,child=child,childfd=childfd,birth=boot+':'+row(pid)['ticks'],childticks=row(child)['ticks'],marker=marker)
    raise RuntimeError('fixture startup failed')
try:
    assert os.getpid()==1,'repro must run in its OWN isolated PID namespace'
    # All signalling/cleanup targets are this fixture's children, through held pidfds.
    owned=spawn_tree('review-owned-marker',100)
    signal_fd(owned['fd'],signal.SIGKILL); wait_for(lambda:ready(owned['fd'])); os.waitpid(owned['pid'],0)
    assert row(owned['child'])['sid']==owned['pid']
    announced_r,announced_w=os.pipe(); go_r,go_w=os.pipe()
    paused=original.replace('    for p in list(held): freeze(p)',
        "    os.write(int(sys.argv[5]),b'P')\n    assert os.read(int(sys.argv[6]),1)==b'G'\n    for p in list(held): freeze(p)")
    assert paused!=original
    helper=subprocess.Popen([sys.executable,'-c',paused,str(owned['pid']),owned['birth'],owned['marker'],json.dumps([[owned['child'],owned['childticks']]]),str(announced_w),str(go_r)],
        pass_fds=(announced_w,go_r),stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True)
    helperfd=hold(helper.pid); children.append(helper.pid); os.close(announced_w); os.close(go_r)
    assert select.select([announced_r],[],[],3)[0] and os.read(announced_r,1)==b'P'
    # Cached anchor has been pinned but not frozen. Its current SID changes.
    signal_fd(owned['childfd'],signal.SIGUSR1)
    wait_for(lambda:row(owned['child'])['sid']==owned['child'])
    time.sleep(.02)
    # Only this private PID namespace's allocator is changed. No host PID allocator is touched.
    open('/proc/sys/kernel/ns_last_pid','w').write(str(owned['pid']-1))
    unrelated=subprocess.Popen([sys.executable,'-c',"import signal,time;signal.signal(signal.SIGHUP,signal.SIG_IGN);time.sleep(600)"],start_new_session=True)
    unrelatedfd=hold(unrelated.pid); children.append(unrelated.pid)
    assert unrelated.pid==owned['pid'] and row(unrelated.pid)['sid']==owned['pid']
    os.write(go_w,b'G'); os.close(go_w); os.close(announced_r)
    out,err=helper.communicate(timeout=8)
    sid_result=dict(helper_exit=helper.returncode,unrelated_new_session_killed=ready(unrelatedfd),proven_child_killed=ready(owned['childfd']),stderr=err.decode()[-300:])
    # Independently exercise the real SIGALRM boundary AFTER committed becomes true.
    second=spawn_tree('review-alarm-marker')
    untouched=spawn_tree('review-unrelated-marker')
    alarm_script=original.replace('    committed = True\n','    committed = True\n    signal.raise_signal(signal.SIGALRM)\n')
    assert alarm_script!=original
    alarm=subprocess.Popen([sys.executable,'-c',alarm_script,str(second['pid']),second['birth'],second['marker'],'[]'],stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True)
    alarmfd=hold(alarm.pid);children.append(alarm.pid)
    out,err=alarm.communicate(timeout=8)
    timeout_result=dict(helper_exit=alarm.returncode,root_alive=not ready(second['fd']),child_alive=not ready(second['childfd']),root_state=row(second['pid'])['state'],child_state=row(second['child'])['state'],unrelated_alive=not ready(untouched['fd']) and not ready(untouched['childfd']),stderr=err.decode()[-300:])
    print(json.dumps(dict(sid_reuse=sid_result,post_commit_alarm=timeout_result)))
finally:
    # Held descriptors address our exact children even for the deliberately reused PID.
    for fd in held:
        try: signal_fd(fd,signal.SIGKILL)
        except ProcessLookupError: pass
    for pid in set(children):
        try: os.waitpid(pid,0)
        except ChildProcessError: pass
    for fd in held+masters:
        try: os.close(fd)
        except OSError: pass
`;
