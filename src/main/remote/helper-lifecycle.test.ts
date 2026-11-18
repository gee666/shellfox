import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { z } from 'zod';
const mocks = vi.hoisted(() => ({ spawn: vi.fn() }));
vi.mock('node:child_process', () => ({ spawn: mocks.spawn }));
import { RemoteHelper } from './helper';

function fixture() {
  const child = Object.assign(new EventEmitter(), { stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough(), kill: vi.fn(() => true) });
  const requests: Array<{ id: string; method: string }> = [];
  child.stdin.on('data', chunk => {
    const request = z.object({ id: z.string(), method: z.string() }).parse(JSON.parse(chunk.toString()));
    requests.push(request);
    child.stdout.write(JSON.stringify({ id: request.id, ok: true, value: {} }) + '\n');
  });
  mocks.spawn.mockReturnValue(child);
  return { child, requests };
}
beforeEach(() => { mocks.spawn.mockReset(); vi.useFakeTimers(); });
afterEach(() => { vi.useRealTimers(); });

it('dispose waits for the original close, not stop acknowledgement, kill or exit', async () => {
  const { child, requests } = fixture();
  const helper = new RemoteHelper('owned-helper');
  await helper.command('start', {});
  let done = false;
  const stopping = helper.dispose().then(() => { done = true; });
  await vi.advanceTimersByTimeAsync(0);
  expect(requests.map(request => request.method)).toEqual(['start', 'stop']);
  expect(child.kill).toHaveBeenCalledOnce();
  child.emit('exit', 0);
  await vi.advanceTimersByTimeAsync(0);
  expect(done).toBe(false);
  await expect(helper.command('start', {})).rejects.toThrow('stopping');
  expect(mocks.spawn).toHaveBeenCalledOnce();
  child.emit('close', 0);
  await stopping;
  expect(done).toBe(true);
});

it('retains the original child after a bounded uncertain close and allows shutdown retry only', async () => {
  const { child } = fixture();
  const helper = new RemoteHelper('owned-helper');
  await helper.command('start', {});
  const stopping = expect(helper.dispose()).rejects.toThrow('close is unconfirmed');
  await vi.advanceTimersByTimeAsync(5001);
  await stopping;
  await expect(helper.command('start', {})).rejects.toThrow('stopping');
  expect(mocks.spawn).toHaveBeenCalledOnce();
  const retry = helper.dispose();
  child.emit('close', 0);
  await retry;
});

it('does not forget a child on error or spawn another until the original close', async () => {
  const { child } = fixture();
  const helper = new RemoteHelper('owned-helper');
  await helper.command('start', {});
  child.emit('error', new Error('pipe failure'));
  await expect(helper.command('start', {})).rejects.toThrow('original process closes');
  expect(mocks.spawn).toHaveBeenCalledOnce();
  const stopping = helper.dispose();
  child.emit('close', 1);
  await stopping;
});

it('kills the retained child after a stop timeout, then still requires close', async () => {
  const { child } = fixture();
  const helper = new RemoteHelper('owned-helper');
  await helper.command('start', {});
  child.stdin.removeAllListeners('data');
  const stopping = helper.dispose();
  await vi.advanceTimersByTimeAsync(2001);
  expect(child.kill).toHaveBeenCalledOnce();
  child.emit('close', 0);
  await stopping;
  expect(vi.getTimerCount()).toBe(0);
});

it('deduplicates verified preparation and passes expected digest arguments to the only spawn', async () => {
  const { child } = fixture();
  const prepare = vi.fn(async () => ({ executable: 'stable-helper', args: ['--expected-sha256', 'a'.repeat(64)] }));
  const helper = new RemoteHelper(prepare);
  await Promise.all([helper.command('start', {}), helper.command('status', {})]);
  expect(prepare).toHaveBeenCalledOnce();
  expect(mocks.spawn).toHaveBeenCalledExactlyOnceWith('stable-helper', ['--expected-sha256', 'a'.repeat(64)], expect.objectContaining({ windowsHide: true }));
  const stopping = helper.dispose();
  await vi.advanceTimersByTimeAsync(0); child.emit('close', 0); await stopping;
});

it('cannot spawn when shutdown races with pending verified publication', async () => {
  let finish: ((value: { executable: string; args: string[] }) => void) | undefined;
  const helper = new RemoteHelper(() => new Promise(resolve => { finish = resolve; }));
  const command = expect(helper.command('start', {})).rejects.toThrow('stopping');
  const stopping = helper.dispose();
  finish?.({ executable: 'stable-helper', args: [] });
  await Promise.all([command, stopping]);
  expect(mocks.spawn).not.toHaveBeenCalled();
});

it('retains uncertain preparation and blocks restart until shutdown can be retried', async () => {
  let finish: ((value: { executable: string; args: string[] }) => void) | undefined;
  const prepare = vi.fn(() => new Promise<{ executable: string; args: string[] }>(resolve => { finish = resolve; }));
  const helper = new RemoteHelper(prepare);
  const command = expect(helper.command('start', {})).rejects.toThrow('stopping');
  const stopping = expect(helper.dispose()).rejects.toThrow('close is unconfirmed');
  await vi.advanceTimersByTimeAsync(5001); await stopping;
  await expect(helper.command('start', {})).rejects.toThrow('stopping');
  expect(prepare).toHaveBeenCalledOnce();
  finish?.({ executable: 'stable-helper', args: [] });
  await command;
  await helper.dispose();
  expect(mocks.spawn).not.toHaveBeenCalled();
});

it('does not launch late when all commands timed out during preparation', async () => {
  let finish: ((value: { executable: string; args: string[] }) => void) | undefined;
  const helper = new RemoteHelper(() => new Promise(resolve => { finish = resolve; }));
  const command = expect(helper.command('start', {})).rejects.toThrow('timed out');
  await vi.advanceTimersByTimeAsync(30001); await command;
  finish?.({ executable: 'stable-helper', args: [] });
  await vi.advanceTimersByTimeAsync(0);
  expect(mocks.spawn).not.toHaveBeenCalled();
  await helper.dispose();
});

it('retains an asynchronously launched child when its first write throws', async () => {
  const { child } = fixture();
  vi.spyOn(child.stdin, 'write').mockImplementation(() => { throw new Error('write failed'); });
  const helper = new RemoteHelper(async () => ({ executable: 'stable-helper', args: [] }));
  await expect(helper.command('start', {})).rejects.toThrow('write failed');
  expect(child.kill).toHaveBeenCalledOnce();
  await expect(helper.command('start', {})).rejects.toThrow('original process closes');
  const stopping = helper.dispose(); child.emit('close', 0); await stopping;
  expect(mocks.spawn).toHaveBeenCalledOnce();
});

it('preserves preparation errors and never falls back to a candidate executable', async () => {
  const helper = new RemoteHelper(async () => { throw new Error('Verified helper update is busy'); });
  await expect(helper.command('start', {})).rejects.toThrow('update is busy');
  expect(mocks.spawn).not.toHaveBeenCalled();
  await helper.dispose();
});
