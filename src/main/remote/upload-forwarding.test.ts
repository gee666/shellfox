import { randomUUID } from 'node:crypto';
import { afterEach, expect, it, vi } from 'vitest';
import { failure, success } from '../../shared/contracts';
import { connectHostClient } from '../../host/client';
import { startHostRpc } from '../../host/rpc';
import { RemoteView } from './view';
import { FakeRemoteHelper, remoteFixture } from './test-fixtures';
import { TERMINAL_UPLOAD_CHUNK_BYTES } from '../../shared/terminal-upload-schemas';

const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => { for (const cleanup of cleanups.splice(0).reverse()) await cleanup(); vi.restoreAllMocks(); });
const terminal = () => ({ tabId: randomUUID(), generation: randomUUID(), controlEpoch: 1 });
function idle(signal?: AbortSignal): Promise<never> {
  return new Promise((_resolve, reject) => signal?.addEventListener('abort', () => reject(new Error('closed')), { once: true }));
}
it('validates upload requests and responses on a direct remote view and reports older hosts as unsupported', async () => {
  let reply: unknown = success({ uploadId: randomUUID(), chunkBytes: TERMINAL_UPLOAD_CHUNK_BYTES });
  const send = vi.fn(async (_peer: string, route: string, _body: unknown, signal?: AbortSignal): Promise<unknown> => {
    if (route === '/v1/pair') return success({ paired: true, terminalControl: 'exclusive-v1' });
    if (route === '/v1/events') return idle(signal);
    return route === '/v1/request' ? reply : success({ disconnected: true });
  });
  const view = new RemoteView(() => ({ status: 'connected' }), send, () => {}); cleanups.push(() => view.dispose());
  const input = { peerId: 'host', method: 'beginTerminalUpload', payload: { ...terminal(), fileName: 'a', size: 0 } };
  expect(await view.request({ ...input, payload: { ...input.payload, path: '/tmp/not-allowed' } })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(send).not.toHaveBeenCalled(); expect(await view.request(input)).toEqual(reply);
  reply = success({ uploadId: randomUUID(), chunkBytes: 1 });
  expect(await view.request(input)).toMatchObject({ ok: false });
  reply = failure('AUTH_FAILED', 'This operation is not available remotely.');
  expect(await view.request(input)).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
  reply = failure('AUTH_FAILED', 'Reconnect this remote view.');
  expect(await view.request(input)).toEqual(reply);
  reply = success({ path: '../untrusted', fileName: 'a', size: 0 });
  expect(await view.request({ peerId: 'host', method: 'finishTerminalUpload', payload: { ...terminal(), uploadId: randomUUID() } })).toMatchObject({ ok: false });
});
it('forwards typed upload operations through a desktop/headless HostClient without widening local operations', async () => {
  const f = await remoteFixture(new Map<string, FakeRemoteHelper>(), 'host'); cleanups.push(f.dispose);
  let reply: unknown = success({ uploadId: randomUUID(), chunkBytes: TERMINAL_UPLOAD_CHUNK_BYTES });
  const request = vi.fn(async () => reply);
  vi.spyOn(f.remote, 'createView').mockImplementation(() => ({ request, subscribe: () => () => {}, disconnect: async () => {}, deny: async () => {}, dispose: async () => {} }));
  const rpc = await startHostRpc({ userData: f.directory, service: f.service, remote: f.remote, mode: 'headless', shutdown: async () => {} }); cleanups.push(() => rpc.close());
  const client = await connectHostClient(f.directory); cleanups.push(() => client.close());
  const input = { peerId: 'host', method: 'beginTerminalUpload' as const, payload: { ...terminal(), fileName: 'a', size: 0 } };
  expect(await client.remote.request(input)).toEqual(reply); expect(request).toHaveBeenCalledWith(input);
  reply = success({ uploadId: randomUUID(), chunkBytes: 1 });
  await expect(client.remote.request(input)).rejects.toThrow();
  reply = success({ path: '../untrusted', fileName: 'a', size: 0 });
  await expect(client.remote.request({ peerId: 'host', method: 'finishTerminalUpload', payload: { ...terminal(), uploadId: randomUUID() } })).rejects.toThrow();
});
