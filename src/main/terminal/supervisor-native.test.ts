import { it, expect } from 'vitest';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const sourcePath = new URL('./native/darwin-terminal-supervisor.c', import.meta.url);
const projectRoot = resolve(process.cwd());
const executable = resolve(projectRoot, 'tmp/terminal-native', `darwin-${process.arch}`, 'shellfox-terminal-supervisor');

it('defines a real per-tab shipped native supervisor path and bounded control protocol', async () => {
  expect(executable).toContain('terminal-native');
  expect(executable.endsWith('shellfox-terminal-supervisor')).toBe(true);
  const source = await readFile(sourcePath, 'utf8');
  expect(source).toContain('--capabilities'); expect(source).toContain('--supervise');
  expect(source).toContain('version\\\":1');
  for (const field of ['ownedSession', 'guardians', 'termination', 'supervisorPid', 'shellPid', 'shellBirth', 'shellAlive', 'exitCode', 'reason']) expect(source).toContain(field);
  expect(source).toContain('#define MAX_PIDS 20000'); expect(source).toContain('#define MAX_GROUPS 256');
  expect(source).toContain('#define MAX_REQUEST 128'); expect(source).toContain('#define MAX_REPLY 2048');
  expect(source).toContain('getpeereid(fd'); expect(source).toContain('difference |=');
  expect(source).toContain('length == 74'); expect(source).toContain('length == 73');
  expect(source).toContain('strcmp(canonical, parent)'); expect(source).toContain('dir.st_uid != getuid()');
  expect(source).toContain('(dir.st_mode & 0777) != 0700'); expect(source).toContain('chmod(path, 0600)');
  expect(source).toContain('value.st_ino == socket_inode');
});

it('keeps an immutable live SID anchor and uses joined current-group guardians, never cached group kills', async () => {
  const source = await readFile(sourcePath, 'utf8');
  expect(source).toContain('getsid(0) != supervisor_pid'); expect(source).toContain('getpgrp() != supervisor_pid');
  expect(source).toContain('tcgetsid(STDIN_FILENO) != supervisor_pid');
  expect(source).toContain('setpgid(0, group)'); expect(source).toContain('getpgrp() != group');
  expect(source).toContain('kill(0, SIGSTOP)'); expect(source).toContain('kill(0, SIGCONT)'); expect(source).toContain('kill(0, SIGKILL)');
  const parentSignals = [...source.matchAll(/\bkill\s*\(([^;\n]+)\)/g)].map(m => m[1]!.trim());
  expect(parentSignals.every(s => s.startsWith('0,') || s === 'g->pid, SIGCONT')).toBe(true);
  expect(source).not.toMatch(/\bkillpg\s*\(/); expect(source).not.toMatch(/\bkill\s*\(\s*-/);
  expect(source).toContain('value.pbi_ppid != (uint32_t)supervisor_pid');
  expect(source).toContain('WEXITED | WNOHANG | WNOWAIT'); expect(source).toContain('WSTOPPED');
  expect(source).toContain('non-reaping WUNTRACED semantics');
  expect(source).not.toMatch(/waitpid\s*\(\s*-1/);
  const reap = source.indexOf('static void reap_after_verified_close');
  expect(source.slice(0, reap)).not.toMatch(/\bwaitpid\s*\(/);
  expect(source).toContain('SIGCHLD, on_child_signal');
});

it('uses real identity/image and real member-state checks, with rollback after partial commit', async () => {
  const source = await readFile(sourcePath, 'utf8');
  expect(source).toContain('proc_pidinfo(pid, PROC_PIDTBSDINFO'); expect(source).toContain('proc_listallpids');
  expect(source).toContain('{ CTL_KERN, KERN_PROC, KERN_PROC_PID, pid }');
  expect(source).toContain('length == sizeof(record)'); expect(source).toContain('record.kp_proc.p_pid == pid');
  expect(source).toContain('proc_pidpath(shell_pid'); expect(source).toContain('!strcmp(path, shell_image)');
  expect(source).toContain('pbi_start_tvsec'); expect(source).toContain('pbi_start_tvusec');
  expect(source).toContain('PROC_FLAG_PSUGID'); expect(source).toContain('members[i].info.pbi_status != SSTOP');
  expect(source).toContain('b.pbi_pgid == (uint32_t)supervisor_pid');
  expect(source).toContain('if (!result) {'); expect(source).toContain('rollback_groups()');
  expect(source).not.toContain('if (!committed)');
  expect(source).toContain('Dispatch every R before waiting');
  expect(source).toMatch(/[Kk]eep resuming/);
  expect(source).toContain('session_snapshot(members, &count, deadline) && count == 0');
  expect(source).toContain('closed_state = true');
});



it('restores reversible freezes after unreadable snapshots without relaxing identity or extending the deadline', async () => {
  const source = await readFile(sourcePath, 'utf8');
  const cleanup = source.slice(source.indexOf('static bool cleanup_session('), source.indexOf('static bool authenticate_shell('));
  expect(cleanup).not.toContain('pass < 100');
  expect(cleanup.match(/deadline = now\(\) \+ FREEZE_SECONDS/g)).toHaveLength(1);
  for (const name of ['count', 'frozen_count']) {
    const failedRead = cleanup.slice(cleanup.indexOf(`if (!session_snapshot(members, &${name}, deadline))`));
    expect(failedRead.slice(0, failedRead.indexOf('sleep_tick(); continue;'))).toContain('rollback_groups()');
  }
  expect(source).toContain('pid=%d errno=%d');
  expect(source).toContain('((a.pbi_flags ^ b.pbi_flags) & PROC_FLAG_PSUGID)');
  expect(source).toContain('!readable_member(&a) || !readable_member(&b)');
});

it('confirms the unreaped direct shell exit on both empty-session paths before publishing closed', async () => {
  const source = await readFile(sourcePath, 'utf8');
  const cleanup = source.slice(source.indexOf('static bool cleanup_session('), source.indexOf('static bool authenticate_shell('));
  expect(cleanup.match(/result = confirm_shell_exit\(deadline\)/g)).toHaveLength(2);
  expect(cleanup.match(/closed_state = true/g)).toHaveLength(1);
  expect(cleanup).toContain('if (result) {\n        closed_state = true; last_reason = NULL;');
  const confirm = source.slice(source.indexOf('static bool confirm_shell_exit('), source.indexOf('static bool cleanup_session('));
  expect(confirm).toContain('while (now() < deadline)');
  expect(confirm).toContain('observe_shell();');
  expect(confirm).toContain('if (shell_exited) return true;');
  expect(confirm).not.toContain('deadline =');
  expect(confirm).not.toContain('shell_exited =');
});

it('authenticates readiness only after shell session/group establishment and exec', async () => {
  const source = await readFile(sourcePath, 'utf8');
  const startup = source.slice(source.indexOf('static bool start_shell('), source.indexOf('static void json_string('));
  expect(startup.indexOf('setpgid(0, 0)')).toBeLessThan(startup.indexOf('write(ready[1], "S", 1)'));
  expect(startup).toContain('getsid(0) != supervisor_pid');
  expect(startup).toContain("read_byte(ready[0], &value, now() + 3.0) && value == 'S'");
  expect(startup).toContain('return authenticate_shell(now() + 3.0)');
  const main = source.slice(source.indexOf('int main('));
  expect(main.indexOf('if (!start_shell(')).toBeLessThan(main.indexOf('client_fd = accept('));
});

it('strips child secrets and keeps status JSON off the PTY stream', async () => {
  const source = await readFile(sourcePath, 'utf8');
  expect(source).toContain('SHELLFOX_SUPERVISOR_'); expect(source).toContain('wipe(environ[i], strlen(environ[i]))');
  expect(source).toContain('wipe(token, sizeof(token))');
  expect(source).toContain('close_parent_fds(fd)'); expect(source).toContain('strip_child_secrets()');
  const reply = source.slice(source.indexOf('static void reply('), source.indexOf('static bool authenticate_request'));
  expect(reply).toContain('send_bytes(fd, body'); expect(reply).not.toContain('printf(' + '"');
  expect(reply).not.toContain('token');
  expect(source).toContain('execv(shell_image, arguments)'); expect(source).toContain('tcsetpgrp(STDIN_FILENO, shell_pid)');
  expect(source).toContain('shell_exit_code = event.si_code == CLD_EXITED ? event.si_status');
});
