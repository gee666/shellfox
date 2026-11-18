import { ChildProcess, spawn } from 'node:child_process';
import { mkdir, mkdtemp, realpath, rm } from 'node:fs/promises';
import path from 'node:path';
import { PassThrough } from 'node:stream';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { acquireHostOwnership } from './ownership';

vi.mock('node:child_process', async () => {
  const { unitChildProcess } = await import('../../tests/unit/app-process-model');
  return { ...unitChildProcess, spawn: vi.fn(unitChildProcess.spawn) };
});

// ChildProcess exit precedes stdio close. Drive both events independently so
// this race is deterministic on every OS, without changing kernel lock behavior.
class ProtocolChild extends ChildProcess {
  override stdin = new PassThrough();
  override stdout = new PassThrough();
  override stderr = new PassThrough();
  private ended = false;
  constructor() {
    super();
    this.stdin.once('finish', () => this.close());
  }
  reply(text: string) { this.stdout.write(text); }
  exit() { this.emit('exit', 0, null); }
  close() {
    if (this.ended) return;
    this.ended = true;
    this.stdout.end(); this.stderr.end(); this.emit('close', 0, null);
  }
}

const directories: string[] = [];
beforeEach(() => { vi.mocked(spawn).mockReset(); });
afterEach(async () => { for (const directory of directories.splice(0)) await rm(directory, { recursive: true, force: true }); });
async function acquire(sequence: (child: ProtocolChild) => void) {
  await mkdir('tmp', { recursive: true });
  const directory = await mkdtemp(path.resolve('tmp/ownership-protocol-'));
  directories.push(directory);
  const child = new ProtocolChild();
  const helperPath = path.join(directory, 'synthetic-helper'), canonical = await realpath(directory);
  vi.mocked(spawn).mockImplementationOnce((file, args, options) => {
    expect([file, args, options]).toEqual([helperPath, ['--ownership', canonical], { windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] }]);
    queueMicrotask(() => sequence(child));
    return child;
  });
  return acquireHostOwnership(directory, 'headless', { helperPath });
}

it('accepts a busy reply drained after process exit, including split stdout chunks', async () => {
  await expect(acquire(child => {
    child.exit();
    child.reply('{"locked":');
    queueMicrotask(() => { child.reply('false}\n'); child.close(); });
  })).resolves.toBeNull();
});

it('reports missing confirmation only after stdout and stderr close', async () => {
  let pendingClose: (() => void) | undefined, completed = false;
  const result = acquire(child => {
    child.exit();
    pendingClose = () => child.close();
  }).then(() => { completed = true; }, (error: unknown) => { completed = true; return error; });
  await vi.waitFor(() => expect(pendingClose).toBeTypeOf('function'));
  try { expect(completed).toBe(false); }
  finally { pendingClose?.(); }
  expect(await result).toMatchObject({ message: 'Shellfox ownership helper exited before confirming the lock.' });
});

it('does not treat a delayed locked true reply from an already exited helper as ownership', async () => {
  await expect(acquire(child => {
    child.exit(); child.reply('{"locked":true}\n'); child.close();
  })).rejects.toThrow('ownership helper exited');
});

it('rejects a helper that confirms ownership then dies before the acquisition resolves', async () => {
  await expect(acquire(child => {
    child.reply('{"locked":true}\n'); child.exit(); child.close();
  })).rejects.toThrow('ownership helper exited');
});

it('still reports loss immediately at exit, before an active owner stdout closes', async () => {
  let helper: ProtocolChild | undefined;
  const lock = await acquire(child => { helper = child; child.reply('{"locked":true}\n'); });
  if (!lock || !helper) throw new Error('Synthetic owner did not start');
  const lost = vi.fn(); lock.onLost(lost);
  helper.exit();
  expect(lost).toHaveBeenCalledOnce();
  expect(() => lock.assertOwned()).toThrow('ownership helper exited');
  const late = vi.fn(); lock.onLost(late);
  expect(late).toHaveBeenCalledExactlyOnceWith(lock.lost);
  helper.close();
  await lock.release();
  expect(() => lock.assertOwned()).toThrow('ownership helper exited');
  expect(spawn).toHaveBeenCalledOnce();
});

it('validates a drained reply rather than hiding malformed output behind process exit', async () => {
  await expect(acquire(child => {
    child.exit(); child.reply('{"locked":"no"}\n'); child.close();
  })).rejects.toThrow('Invalid Shellfox ownership helper response');
});

it('rejects a scripted missing helper after the synthetic child closes', async () => {
  await expect(acquire(child => {
    child.emit('error', Object.assign(new Error('missing fixture image'), { code: 'ENOENT' }));
    child.close();
  })).rejects.toThrow('ownership helper could not start');
});

it('times out the synthetic handshake and stays pending until the model closes', async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
  let announce!: (child: ProtocolChild) => void;
  const spawned = new Promise<ProtocolChild>(resolve => { announce = resolve; });
  let settled = false;
  const result = acquire(child => { child.stdin.removeAllListeners('finish'); announce(child); })
    .then(() => { settled = true; return undefined; }, (error: unknown) => { settled = true; return error; });
  try {
    const child = await Promise.race([spawned, result.then(error => { throw error ?? new Error('Fixture closed before spawn'); })]);
    try {
      await vi.advanceTimersByTimeAsync(8000);
      expect(settled).toBe(false);
    } finally { child.close(); }
    expect(await result).toMatchObject({ message: 'Shellfox ownership helper timed out.' });
    expect(spawn).toHaveBeenCalledOnce();
  } finally { vi.useRealTimers(); }
});
