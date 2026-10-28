import { randomUUID } from 'node:crypto';
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import path from 'node:path';
import { afterEach } from 'vitest';
import { RemoteHost } from './host';
import { embeddedFixture, identity, signedHeaders } from './test-fixtures';
import { responseSchemas } from '../../shared/remote-schemas';

const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => { for (const cleanup of cleanups.splice(0).reverse()) await cleanup(); });
export async function uploadFixture(maxBytes?: number) {
  await mkdir('tmp', { recursive: true }); const directory = await mkdtemp(path.resolve('tmp/upload-test-'));
  const root = path.join(directory, 'uploads'), f = await embeddedFixture(), secret = 'upload-test';
  let approved = true;
  const host = new RemoteHost(f.service, secret, () => approved, () => {}, undefined, undefined, { root, maxBytes });
  const url = await host.listen(), clientId = randomUUID();
  cleanups.push(async () => { await host.dispose(); await f.service.dispose(); await rm(directory, { force: true, recursive: true }); });
  const send = async (requestPath: string, payload: unknown, peer = identity, unsigned = false): Promise<unknown> => {
    const body = JSON.stringify(payload);
    const response = await fetch(url + requestPath, { method: 'POST', body, headers: unsigned ? {} : signedHeaders(secret, peer, requestPath, body) }); return response.json();
  };
  const request = (method: string, payload: unknown, client = clientId, peer = identity) => send('/v1/request', { clientId: client, method, payload }, peer);
  await send('/v1/pair', { clientId });
  const created = responseSchemas.createSession.parse(await request('createSession', { cwd: '/work', requestId: randomUUID() }));
  if (!created.ok) throw new Error('Could not create terminal.');
  const tab = created.value.tabs[0];
  const attached = responseSchemas.attachTerminal.parse(await request('attachTerminal', { tabId: tab.id, generation: tab.generation }));
  if (!attached.ok || attached.value.control !== 'owned') throw new Error('Could not own terminal.');
  const terminal = { tabId: tab.id, generation: attached.value.generation, controlEpoch: attached.value.controlEpoch };
  const begin = async (fileName = 'photo.png', size = 0) => {
    const result = responseSchemas.beginTerminalUpload.parse(await request('beginTerminalUpload', { ...terminal, fileName, size }));
    if (!result.ok) throw new Error(result.error.message);
    return { ...terminal, uploadId: result.value.uploadId };
  };
  return { ...f, root, directory, host, clientId, send, request, terminal, begin, revoke: () => { approved = false; host.revoke(identity.id); }, approve: () => { approved = true; } };
}
