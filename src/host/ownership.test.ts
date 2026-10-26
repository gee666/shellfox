import { existsSync } from 'node:fs';
import { mkdtemp, mkdir, rm } from 'node:fs/promises';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { acquireHostOwnership } from './ownership';
import { hostResourcePaths } from './paths';
import { runHost } from './runtime';
import { EmbeddedSessionService } from '../main/terminal/service';
import { PtyBackend } from '../main/terminal/backend';
import { factoryFixture, MemoryRepository } from '../main/terminal/test-fixtures';

vi.mock('node:child_process', async original => { const actual = await original<typeof import('node:child_process')>(); return { ...actual, spawn: vi.fn(actual.spawn) }; });
const cleanup: Array<() => Promise<unknown>> = [];
afterEach(async () => { for (const action of cleanup.splice(0).reverse()) await action(); });
describe.skipIf(!existsSync(hostResourcePaths().tailscale))('native ownership helper, requires build:tailscale', () => {
  it('reports unexpected helper death and lets the OS release the lock', async () => {
    await mkdir('tmp', { recursive: true }); const userData = await mkdtemp(path.resolve('tmp/ownership-loss-'));
    cleanup.push(() => rm(userData, { recursive: true, force: true }));
    const lock = await acquireHostOwnership(userData, 'headless'); if (!lock) throw new Error('Lock unexpectedly busy');
    cleanup.push(() => lock.release());
    const result = vi.mocked(spawn).mock.results.at(-1); if (!result || result.type !== 'return') throw new Error('Missing owned helper');
    const helper = result.value; expect(helper.pid).toBeGreaterThan(0);
    const lost = vi.fn(); lock.onLost(lost); helper.kill();
    await vi.waitFor(() => expect(lost).toHaveBeenCalledOnce());
    expect(lock.lost).toBeInstanceOf(Error); expect(() => lock.assertOwned()).toThrow('ownership helper exited');
    const next = await acquireHostOwnership(userData); expect(next).not.toBeNull(); await next?.release();
  });
  it('stops the foreground runtime and closes its repository when the helper dies', async () => {
    await mkdir('tmp', { recursive: true }); const userData = await mkdtemp(path.resolve('tmp/runtime-ownership-loss-'));
    cleanup.push(() => rm(userData, { recursive: true, force: true }));
    const pty = factoryFixture();
    const service = new EmbeddedSessionService(new MemoryRepository(), new PtyBackend(pty.options), () => ({ setWatch() {}, dispose() {} }));
    cleanup.push(() => service.dispose()); const closeRepository = vi.fn();
    await expect(runHost({ userData, createResources: async () => ({ service, initialize: () => service.initialize(), closeRepository }), onReady() {
      const result = vi.mocked(spawn).mock.results.at(-1); if (!result || result.type !== 'return') throw new Error('Missing owned helper'); result.value.kill();
    } })).rejects.toThrow('ownership helper exited');
    expect(closeRepository).toHaveBeenCalledOnce();
    const next = await acquireHostOwnership(userData); expect(next).not.toBeNull(); await next?.release();
  });
  it('rejects foreground completion and closes SQLite even when fatal PTY cleanup fails', async () => {
    await mkdir('tmp', { recursive: true }); const userData = await mkdtemp(path.resolve('tmp/runtime-fatal-cleanup-'));
    cleanup.push(() => rm(userData, { recursive: true, force: true }));
    const pty = factoryFixture();
    const service = new EmbeddedSessionService(new MemoryRepository(), new PtyBackend(pty.options), () => ({ setWatch() {}, dispose() {} }));
    cleanup.push(service.dispose.bind(service)); const closeRepository = vi.fn();
    vi.spyOn(service, 'dispose').mockRejectedValue(new Error('Unconfirmed test PTY cleanup.'));
    await expect(runHost({ userData, createResources: async () => ({ service, initialize: () => service.initialize(), closeRepository }), onReady() {
      const result = vi.mocked(spawn).mock.results.at(-1); if (!result || result.type !== 'return') throw new Error('Missing owned helper'); result.value.kill();
    } })).rejects.toThrow('could not confirm all cleanup');
    expect(closeRepository).toHaveBeenCalledOnce();
    const next = await acquireHostOwnership(userData); expect(next).not.toBeNull(); await next?.release();
  });
  it('intentional release does not report ownership loss and is idempotent', async () => {
    await mkdir('tmp', { recursive: true }); const userData = await mkdtemp(path.resolve('tmp/ownership-release-'));
    cleanup.push(() => rm(userData, { recursive: true, force: true }));
    const lock = await acquireHostOwnership(userData); if (!lock) throw new Error('Lock unexpectedly busy');
    const lost = vi.fn(); lock.onLost(lost); await Promise.all([lock.release(), lock.release()]);
    expect(lost).not.toHaveBeenCalled(); expect(() => lock.assertOwned()).toThrow('released');
  });
});
it('fails closed if the native lock helper is missing', async () => {
  await mkdir('tmp', { recursive: true }); const userData = await mkdtemp(path.resolve('tmp/ownership-missing-'));
  cleanup.push(() => rm(userData, { recursive: true, force: true }));
  await expect(acquireHostOwnership(userData, 'headless', { helperPath: path.join(userData, 'missing-helper') })).rejects.toThrow('could not start');
});
