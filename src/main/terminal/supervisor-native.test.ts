import { it, expect } from 'vitest';
import { readFile, mkdir, mkdtemp, realpath, chmod, rm } from 'node:fs/promises';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { resolve, join } from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { randomBytes, randomUUID } from 'node:crypto';
import { createConnection } from 'node:net';
import { stripVTControlCharacters } from 'node:util';

const sourcePath = new URL('./native/darwin-terminal-supervisor.c', import.meta.url);
const projectRoot = resolve(process.cwd());
const executable = resolve(projectRoot, 'tmp/terminal-native', `darwin-${process.arch}`, 'shellfox-terminal-supervisor');
const native = process.platform === 'darwin' && process.env.SHELLFOX_DARWIN_SUPERVISOR_NATIVE === '1';
const run = promisify(execFile);

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

type Status = { version: number; ok: boolean; state: string; supervisorPid: number; shellPid: number; shellBirth: string; shellAlive: boolean; exitCode: number | null; reason: string | null };
function request(socket: string, token: string, command: 'STATUS' | 'CLOSE'): Promise<Status> {
  return new Promise((resolveStatus, reject) => {
    const connection = createConnection(socket);
    let output = '', finished = false;
    const timer = setTimeout(() => finish(new Error('supervisor control deadline')), 15000);
    function finish(error?: Error) {
      if (finished) return; finished = true; clearTimeout(timer); connection.destroy();
      if (error) reject(error);
      else { try { resolveStatus(JSON.parse(output)); } catch { reject(new Error('invalid native reply')); } }
    }
    connection.on('connect', () => connection.write(`1\t${token}\t${command}\n`));
    connection.on('data', data => { output += data.toString('utf8'); if (Buffer.byteLength(output) > 2048) finish(new Error('oversized native reply')); else if (output.endsWith('\n')) finish(); });
    connection.on('error', finish); connection.on('end', () => finish(output ? undefined : new Error('control connection refused')));
  });
}

/* Opt-in, actual native macOS acceptance. This Windows host only runs the source
 * regression checks above. A missing compiler/addon or false preflight FAILS here,
 * not a fake success or a deliberate permanently unavailable profile. */
it.runIf(native)('native Darwin builds supervisor and verifies real PTY job-control cleanup/isolation/exit 7', async () => {
  await mkdir(resolve(projectRoot, 'tmp/terminal-native', `darwin-${process.arch}`), { recursive: true });
  await run('xcrun', ['clang', '-std=c11', '-O2', '-Wall', '-Wextra', '-Werror', fileURLToPath(sourcePath), '-lproc', '-o', executable], { timeout: 30000, maxBuffer: 1024 * 1024 });
  await chmod(executable, 0o755);
  const cap = await run(executable, ['--capabilities'], { timeout: 5000, maxBuffer: 16384 });
  expect(JSON.parse(cap.stdout)).toMatchObject({ version: 1, platform: 'darwin', arch: process.arch, available: true, ownedSession: true, guardians: true, termination: true });
  const require = createRequire(import.meta.url);
  const pty = require('node-pty') as { spawn(file: string, args: string[], options: object): { pid: number; write(data: string): void; onData(fn: (data: string) => void): { dispose(): void }; onExit(fn: (e: { exitCode: number }) => void): { dispose(): void } } };
  const shell = await realpath('/bin/bash');
  const root = await realpath(projectRoot);
  const active: Array<{ socket: string; nonce: string; directory: string; ended: boolean; output: string; process: ReturnType<typeof pty.spawn>; exit: Promise<number> }> = [];
  async function launch() {
    const directory = await mkdtemp(join(root, 'tmp', 'sv-')); await chmod(directory, 0o700);
    const socket = join(directory, 'c'); expect(Buffer.byteLength(socket)).toBeLessThan(104);
    const nonce = randomBytes(32).toString('hex');
    const proc = pty.spawn(executable, ['--supervise', socket, shell, '--noprofile', '--norc', '-i'], { name: 'xterm-256color', cols: 80, rows: 24, cwd: root,
      env: { ...process.env, SHELLFOX_SUPERVISOR_TOKEN: nonce, SHELLFOX_TERMINAL_MARKER: randomUUID(), SHELLFOX_SUPERVISOR_SENTINEL: 'must-not-leak' } });
    const entry = { socket, nonce, directory, ended: false, output: '', process: proc, exit: Promise.resolve(0) };
    entry.exit = new Promise<number>(resolveExit => proc.onExit(e => { entry.ended = true; resolveExit(e.exitCode); }));
    proc.onData(data => { entry.output = (entry.output + data).slice(-256 * 1024); }); active.push(entry);
    let status: Status | undefined;
    const deadline = Date.now() + 8000;
    while (Date.now() < deadline) {
      try { status = await request(socket, nonce, 'STATUS'); if (status.ok && status.shellBirth) break; } catch { /* startup socket race only */ }
      await new Promise(resolveTick => setTimeout(resolveTick, 10));
    }
    expect(status).toMatchObject({ version: 1, ok: true, state: 'open', supervisorPid: proc.pid, shellAlive: true });
    expect(status!.shellBirth).toMatch(/^darwin:\d+:\d{6}$/);
    expect(status!.shellPid).not.toBe(proc.pid);
    return entry;
  }
  async function waitOutput(entry: typeof active[number], pattern: RegExp) {
    const deadline = Date.now() + 5000;
    while (Date.now() < deadline) { if (pattern.test(stripVTControlCharacters(entry.output))) return; await new Promise(resolveTick => setTimeout(resolveTick, 10)); }
    throw new Error('actual PTY output marker not observed');
  }
  try {
    const owned = await launch(), unrelated = await launch();
    await expect(request(owned.socket, '0'.repeat(64), 'STATUS')).rejects.toThrow();
    owned.process.write(`printf 'SECRET_CHECK:<%s>:<%s>\\n' "$SHELLFOX_SUPERVISOR_TOKEN" "$SHELLFOX_SUPERVISOR_SENTINEL"\r`);
    await waitOutput(owned, /SECRET_CHECK:<>:<>/);
    expect(owned.output).not.toContain(owned.nonce);
    // Distinct job-control PGIDs. HUP ignoring is inherited by both sleeps.
    owned.process.write(`trap '' HUP; sleep 600 & sleep 600 & printf 'JOBS_READY\\n'\r`);
    await waitOutput(owned, /JOBS_READY\r?\n/);
    const closed = await request(owned.socket, owned.nonce, 'CLOSE');
    expect(closed).toMatchObject({ version: 1, ok: true, state: 'closed', shellAlive: false });
    await owned.exit;
    expect((await request(unrelated.socket, unrelated.nonce, 'STATUS')).shellAlive).toBe(true);
    unrelated.process.write('exit 7\r');
    expect(await unrelated.exit).toBe(7);
  } finally {
    // Only authenticated owned supervisor control, never test cleanup by raw PID.
    for (const entry of active) {
      if (!entry.ended) {
        const result = await request(entry.socket, entry.nonce, 'CLOSE');
        if (!result.ok || result.state !== 'closed') throw new Error('native supervisor cleanup unconfirmed; endpoint retained');
        await entry.exit;
      }
      await rm(entry.directory, { recursive: true, force: true });
    }
  }
}, 90000);
