import { afterEach, describe, expect, it, vi } from 'vitest';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { AutomationRuns } from './automation';
import type { AutomationStart } from '../shared/automation-schemas';

const managers: AutomationRuns[] = [];
afterEach(async () => {
  try { await Promise.all(managers.splice(0).map(manager => manager.close())); }
  finally { vi.useRealTimers(); }
});
function fixture() {
  const manager = new AutomationRuns({ focusSession: vi.fn() }, path.resolve('tmp')); managers.push(manager);
  const start = (script: string, detached = false, owner = 'owner'): Promise<ReturnType<AutomationRuns['list']>[number]> => {
    const env: Record<string, string> = {};
    for (const [key, value] of Object.entries(process.env)) if (value !== undefined) env[key] = value;
    const input: AutomationStart = { requestId: randomUUID(), executable: process.execPath, args: ['-e', script], cwd: process.cwd(), env, detached };
    return manager.start(input, owner);
  };
  return { manager, start };
}
async function collect(manager: AutomationRuns, id: string, owner = 'owner') {
  let after = 0; const stdout: Buffer[] = [], stderr: Buffer[] = [];
  for (let attempts = 0; attempts < 1000; attempts++) {
    const result = manager.read(id, after, owner);
    for (const frame of result.frames) { (frame.stream === 'stdout' ? stdout : stderr).push(Buffer.from(frame.data, 'base64')); after = frame.sequence; }
    if (!result.frames.length && result.run.state === 'exited') return { stdout: Buffer.concat(stdout), stderr: Buffer.concat(stderr), code: result.run.exitCode };
    await delay(10);
  }
  throw new Error('Process did not finish.');
}
describe('host automation pipe processes', () => {
  it('preserves non-UTF8 stdout, separate stderr, stdin bytes and exit status', async () => {
    const { manager, start } = fixture();
    const run = await start("process.stdin.on('data', b=>process.stdout.write(b)); process.stdin.on('end',()=>{process.stderr.write(Buffer.from([0,255,10]));process.exitCode=7;});");
    await manager.write(run.id, Buffer.from([0, 255, 13, 10, 128]).toString('base64'), true, 'owner');
    const result = await collect(manager, run.id);
    expect(result.stdout).toEqual(Buffer.from([0, 255, 13, 10, 128])); expect(result.stderr).toEqual(Buffer.from([0, 255, 10])); expect(result.code).toBe(7);
  });
  it('does not give the process a TTY or mutate its pipe data for a viewer', async () => {
    const { manager, start } = fixture();
    const run = await start("process.stdout.write(JSON.stringify({tty:!!process.stdout.isTTY,cwd:process.cwd()}));");
    const result = await collect(manager, run.id);
    expect(JSON.parse(result.stdout.toString())).toEqual({ tty: false, cwd: process.cwd() });
  });
  it('preserves large output with bounded backpressure', async () => {
    const { manager, start } = fixture();
    const run = await start("process.stdout.write(Buffer.alloc(3*1024*1024, 123));");
    await delay(100);
    const result = await collect(manager, run.id);
    expect(result.stdout.length).toBe(3 * 1024 * 1024); expect(result.stdout.every(byte => byte === 123)).toBe(true);
  });
  it('closes detached stdin and keeps jobs available after the starting client leaves', async () => {
    const { manager, start } = fixture();
    const run = await start("process.stdin.resume();process.stdin.on('end',()=>process.exit(0));", true);
    for (let i = 0; manager.list()[0].state !== 'exited' && i < 100; i++) await delay(10);
    expect(manager.list()).toMatchObject([{ id: run.id, state: 'exited', exitCode: 0 }]);
    expect(() => manager.read(run.id, 0, 'owner')).toThrow('Pipe collection ended');
  });
  it('rejects another client reading or writing the original pipes', async () => {
    const { manager, start } = fixture(); const run = await start('setInterval(()=>{},1000)');
    expect(() => manager.read(run.id, 0, 'other')).toThrow('starting client');
    await expect(manager.write(run.id, '', true, 'other')).rejects.toThrow('starting client');
    await manager.stop(run.id); expect(manager.list()[0].state).toBe('exited');
  });
  it('lease expiry ends stdin and drains both output pipes so an abandoned run can exit', async () => {
    vi.useFakeTimers({ toFake: ['Date', 'setInterval', 'clearInterval'] });
    const { manager, start } = fixture();
    const run = await start("process.stdout.write(Buffer.alloc(2*1024*1024));process.stderr.write(Buffer.alloc(2*1024*1024));process.stdin.resume();process.stdin.on('end',()=>{process.exitCode=0;});");
    await delay(100);
    await vi.advanceTimersByTimeAsync(35_000);
    expect(() => manager.read(run.id, 0, 'owner')).toThrow('Pipe collection ended');
    await expect(manager.write(run.id, '', true, 'owner')).rejects.toThrow('stdin is closed');
    for (let i = 0; manager.list()[0].state !== 'exited' && i < 200; i++) await delay(10);
    expect(manager.list()[0]).toMatchObject({ state: 'exited', exitCode: 0 });
  });
  it('releasing an owner is idempotent, ends all its inputs and leaves other owners alone', async () => {
    const { manager, start } = fixture();
    const script = "process.stdin.resume();process.stdin.on('end',()=>{process.stdout.write('EOF');process.exitCode=0;});";
    const first = await start(script), second = await start(script), other = await start(script, false, 'other');
    manager.releaseOwner('unknown'); manager.releaseOwner('owner'); manager.releaseOwner('owner');
    for (const run of [first, second]) {
      expect(() => manager.read(run.id, 0, 'owner')).toThrow('Pipe collection ended');
      await expect(manager.write(run.id, '', true, 'owner')).rejects.toThrow('stdin is closed');
    }
    for (let i = 0; manager.list().filter(run => run.id !== other.id).some(run => run.state !== 'exited') && i < 200; i++) await delay(10);
    expect(manager.list().filter(run => run.id !== other.id)).toMatchObject([{ state: 'exited', exitCode: 0 }, { state: 'exited', exitCode: 0 }]);
    expect(manager.read(other.id, 0, 'other').run.state).toBe('running');
    await manager.write(other.id, '', true, 'other');
    expect((await collect(manager, other.id, 'other')).stdout.toString()).toBe('EOF');
  });
  it('reports launch errors without retaining a phantom run', async () => {
    const { manager } = fixture();
    await expect(manager.start({ requestId: randomUUID(), executable: 'shellfox-nonexistent-executable', args: [], cwd: process.cwd(), env: {}, detached: false }, 'owner')).rejects.toThrow();
    expect(manager.list()).toEqual([]);
  });
});
