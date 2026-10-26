import { createServer } from 'node:net';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { randomBytes } from 'node:crypto';
import path from 'node:path';
import { afterEach, expect, it } from 'vitest';
import { publishControlEndpoint } from '../main/control-endpoint';
import { legacyHostIsRunning } from './legacy-owner';

const cleanup: Array<() => Promise<unknown>> = [];
afterEach(async () => { for (const action of cleanup.splice(0).reverse()) await action(); });
async function directory() { await mkdir('tmp', { recursive: true }); const dir = await mkdtemp(path.resolve('tmp/legacy-owner-')); cleanup.push(() => rm(dir, { force: true, recursive: true })); return dir; }
it('allows absent endpoints and explicitly refused stale listener ports', async () => {
  const dir = await directory(); expect(await legacyHostIsRunning(dir)).toBe(false);
  const server = createServer(); await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  const address = server.address(); if (!address || typeof address === 'string') throw new Error('No listener');
  await new Promise<void>(resolve => server.close(() => resolve()));
  const remove = await publishControlEndpoint(dir, { version: 1, host: '127.0.0.1', port: address.port, secret: randomBytes(32).toString('hex') }); cleanup.push(remove);
  expect(await legacyHostIsRunning(dir)).toBe(false);
});
it('fails closed for an unreadable or invalid old endpoint', async () => {
  const dir = await directory(); await mkdir(path.join(dir, 'control')); await writeFile(path.join(dir, 'control/endpoint.json'), '{}');
  await expect(legacyHostIsRunning(dir)).rejects.toThrow('unsafe or unreadable');
});
it('fails closed for a live legacy listener that never authenticates or replies', async () => {
  const dir = await directory();
  const sockets = new Set<import('node:net').Socket>();
  const server = createServer(socket => { sockets.add(socket); socket.on('error', () => {}); });
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  cleanup.push(async () => { for (const socket of sockets) socket.destroy(); await new Promise<void>(resolve => server.close(() => resolve())); });
  const address = server.address(); if (!address || typeof address === 'string') throw new Error('No listener');
  const remove = await publishControlEndpoint(dir, { version: 1, host: '127.0.0.1', port: address.port, secret: randomBytes(32).toString('hex') }); cleanup.push(remove);
  await expect(legacyHostIsRunning(dir)).rejects.toThrow('could not be verified');
}, 10_000);
