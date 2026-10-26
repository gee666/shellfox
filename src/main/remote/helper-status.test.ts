import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import { afterEach, expect, it, vi } from 'vitest';
import { z } from 'zod';
const mocks = vi.hoisted(() => ({ spawn: vi.fn() }));
vi.mock('node:child_process', () => ({ spawn: mocks.spawn }));
import { RemoteHelper } from './helper';

const cleanups: Array<() => Promise<void>> = [];
afterEach(async () => { for (const cleanup of cleanups.splice(0)) await cleanup(); vi.clearAllMocks(); });
it('validates native status events independently of command replies and isolates observers', async () => {
  const child = Object.assign(new EventEmitter(), { stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough(), kill: vi.fn() }); mocks.spawn.mockReturnValue(child);
  const commands: string[] = []; child.stdin.on('data', chunk => {
    const text = chunk.toString(); commands.push(text);
    const request = z.object({ id: z.string(), method: z.string() }).parse(JSON.parse(text));
    if (request.method === 'stop') child.stdout.write(JSON.stringify({ id: request.id, ok: true, value: {} }) + '\n');
  });
  const helper = new RemoteHelper('test-helper'); cleanups.push(() => helper.dispose());
  const listener = vi.fn(); helper.subscribeStatus(() => { throw new Error('Bad observer'); }); helper.subscribeStatus(listener);
  const pending = helper.command('status', {});
  child.stdout.write(JSON.stringify({ event: 'status', payload: { status: 'login-required', loginUrl: 'http://unsafe', error: null } }) + '\n');
  expect(listener).not.toHaveBeenCalled();
  child.stdout.write(JSON.stringify({ event: 'status', payload: { status: 'login-required', loginUrl: 'https://login.tailscale.com/a/test', error: null } }) + '\n');
  expect(listener).toHaveBeenLastCalledWith({ status: 'login-required', loginUrl: 'https://login.tailscale.com/a/test', error: null });
  child.stdout.write(JSON.stringify({ event: 'status', payload: { status: 'online', loginUrl: 'https://login.tailscale.com/a/old', error: null } }) + '\n');
  expect(listener).toHaveBeenLastCalledWith({ status: 'online', loginUrl: null, error: null });
  const request = z.object({ id: z.string() }).parse(JSON.parse(commands[0]));
  child.stdout.write(JSON.stringify({ id: request.id, ok: true, value: 'reply-not-event' }) + '\n');
  expect(await pending).toBe('reply-not-event');
  child.emit('exit', 1);
  expect(listener).toHaveBeenLastCalledWith({ status: 'error', loginUrl: null, error: 'Tailscale helper stopped.' });
  await expect(helper.command('status', {})).rejects.toThrow('Disable and re-enable');
  expect(mocks.spawn).toHaveBeenCalledOnce();
});
