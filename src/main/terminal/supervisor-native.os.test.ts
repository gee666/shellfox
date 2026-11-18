import { it, expect } from 'vitest';
import { mkdir, mkdtemp, realpath, chmod, rm } from 'node:fs/promises';
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
// OWNED INTEGRATION HOLD: this file is never selected by default unit.
it.runIf(process.platform !== 'win32')('executes production C snapshot/cleanup control flow against deterministic OS races', async () => {
  const result = await run(process.execPath, ['scripts/darwin-supervisor-model-test.mjs'], { cwd: projectRoot, timeout: 60000, maxBuffer: 1024 * 1024 });
  expect(result.stdout).toContain('failed frozen read restores groups');
  expect(result.stdout).toContain('persistent denial and failed rollback never commit');
  expect(result.stdout).toContain('birth/SID/group/credential changes');
  expect(result.stdout).toContain('zombie and post-commit empty snapshots wait for direct-child exit');
  expect(result.stdout).toContain('missing exit events and waitid errors retain ownership');
}, 60000);
it.runIf(native)('native Darwin builds supervisor and verifies real PTY job-control cleanup/isolation/exit 7', async () => {
  await mkdir(resolve(projectRoot, 'tmp/terminal-native', `darwin-${process.arch}`), { recursive: true });
  await run('xcrun', ['clang', '-std=c11', '-O2', '-Wall', '-Wextra', '-Werror', fileURLToPath(sourcePath), '-lproc', '-o', executable], { timeout: 30000, maxBuffer: 1024 * 1024 });
  await chmod(executable, 0o755);
  const cap = await run(executable, ['--capabilities'], { timeout: 5000, maxBuffer: 16384 });
  expect(JSON.parse(cap.stdout)).toMatchObject({ version: 1, platform: 'darwin', arch: process.arch, available: true, ownedSession: true, guardians: true, termination: true });
  const require = createRequire(import.meta.url);
  const { verifyBuiltUnixPty } = require('../../../scripts/pty-runtime.cjs');
  const runtime = verifyBuiltUnixPty(require);
  console.log('Darwin lifecycle PTY prerequisite:', { ...runtime, node: process.versions.node, napi: process.versions.napi });
  const pty = require('node-pty') as { spawn(file: string, args: string[], options: object): { pid: number; write(data: string): void; onData(fn: (data: string) => void): { dispose(): void }; onExit(fn: (e: { exitCode: number }) => void): { dispose(): void } } };
  const shell = await realpath('/bin/bash');
  const root = await realpath(projectRoot);
  const active: Array<{ socket: string; nonce: string; directory: string; ended: boolean; output: string; process: ReturnType<typeof pty.spawn>; exit: Promise<number> }> = [];
  async function launch(image = shell, args = ['--noprofile', '--norc', '-i']) {
    const directory = await mkdtemp(join(root, 'tmp', 'sv-')); await chmod(directory, 0o700);
    const socket = join(directory, 'c'); expect(Buffer.byteLength(socket)).toBeLessThan(104);
    const nonce = randomBytes(32).toString('hex');
    const proc = pty.spawn(executable, ['--supervise', socket, image, ...args], { name: 'xterm-256color', cols: 80, rows: 24, cwd: root,
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
    // Match packaged profile discovery, including macOS login startup helpers.
    // Each immediate close must pass on its first authenticated request.
    const zsh = await realpath('/bin/zsh');
    for (let index = 0; index < 40; index++) {
      const login = await launch(zsh, ['-l', '-i']);
      const result = await request(login.socket, login.nonce, 'CLOSE');
      expect(result).toMatchObject({ ok: true, state: 'closed', shellAlive: false });
      expect(result.exitCode).not.toBeNull();
      expect(await login.exit).toBe(result.exitCode);
    }
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
}, 180000);
