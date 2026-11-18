import { afterEach, describe, expect, it, vi } from 'vitest';
import { rpcTestFixture } from './rpc-test-fixture';
import { acquireHostOwnership, isHostRunning } from './ownership';
import { HostRuntime } from './runtime';
import { startControlServer } from '../main/control-server';

const cleanup: Array<() => Promise<unknown>> = [];
afterEach(async () => { for (const action of cleanup.splice(0).reverse()) await action(); });
const { directory, fixture } = rpcTestFixture(cleanup);
// OWNED INTEGRATION HOLD: this file is never selected by default unit.
describe('host ownership and lifecycle, requires build:tailscale', () => {
  it('has exactly one winner for concurrent acquisition and releases without stale files', async () => {
    const userData = await directory();
    const results = await Promise.allSettled(Array.from({ length: 8 }, () => acquireHostOwnership(userData)));
    const owners = results.flatMap(result => result.status === 'fulfilled' && result.value ? [result.value] : []);
    for (const owner of owners) cleanup.push(() => owner.release());
    expect(results.filter(result => result.status === 'rejected')).toEqual([]);
    expect(owners).toHaveLength(1);
    expect(await isHostRunning(userData)).toBe(true); await owners[0].release();
    expect(await isHostRunning(userData)).toBe(false);
  });
  it('refuses a live authenticated legacy desktop before opening SQLite', async () => {
    const f = await fixture(); const control = await startControlServer(f.userData, f.service, { active: false, set: active => active }); cleanup.push(() => control.dispose());
    expect(await acquireHostOwnership(f.userData)).toBeNull();
  });
  it('waits for confirmed own-terminal cleanup before closing repository and releasing ownership', async () => {
    const f = await fixture(); await f.rpc.close();
    const calls: string[] = []; let confirm = false;
    const originalDispose = f.service.dispose.bind(f.service);
    const runtime = await HostRuntime.create({ userData: f.userData, createResources: async () => ({
      service: Object.assign(f.service, { dispose: async () => { calls.push('pty'); if (!confirm) throw new Error('unconfirmed cleanup'); await originalDispose(); } }),
      initialize: async () => {}, closeRepository: () => { calls.push('sqlite'); },
    }) });
    await expect(runtime.stop()).rejects.toThrow('unconfirmed'); expect(calls).toEqual(['pty']);
    const client = await f.connect(); expect((await client.service.getSnapshot()).ok).toBe(true);
    await expect(client.stop()).rejects.toThrow('unconfirmed');
    expect((await client.service.getSnapshot()).ok).toBe(true);
    expect(await isHostRunning(f.userData)).toBe(true); confirm = true;
    let notified = false; runtime.onStopped(() => { notified = true; });
    await client.stop(); await vi.waitFor(() => expect(notified).toBe(true));
    expect(calls).toEqual(['pty', 'pty', 'pty', 'sqlite']); expect(await isHostRunning(f.userData)).toBe(false);
  });
});
