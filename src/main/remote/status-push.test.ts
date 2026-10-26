import { afterEach, expect, it, vi } from 'vitest';
import { FakeRemoteHelper, remoteFixture } from './test-fixtures';
import type { HelperStatus } from './helper';

const cleanups: Array<() => Promise<void>> = [];
afterEach(async () => { for (const cleanup of cleanups.splice(0).reverse()) await cleanup(); });
it('publishes login status immediately, clears a stale online URL and fences an older status reply', async () => {
  const client = await remoteFixture(new Map<string, FakeRemoteHelper>(), 'client'); cleanups.push(client.dispose);
  const command = client.helper.command.bind(client.helper); let finish: ((value: unknown) => void) | undefined, started = false;
  const pending = new Promise<unknown>(resolve => { finish = resolve; });
  vi.spyOn(client.helper, 'command').mockImplementation((method, payload) => { if (method === 'status') { started = true; return pending; } return command(method, payload); });
  const login = client.remote.login({}); await vi.waitFor(() => expect(started).toBe(true));
  try {
    client.helper.emitStatus({ status: 'login-required', loginUrl: 'https://login.tailscale.com/a/new', error: null });
    expect(client.remote.getState()).toMatchObject({ ok: true, value: { status: 'login-required', loginUrl: 'https://login.tailscale.com/a/new' } });
    client.helper.emitStatus({ status: 'online', loginUrl: 'https://login.tailscale.com/a/stale', error: null });
    expect(client.remote.getState()).toMatchObject({ ok: true, value: { status: 'online', loginUrl: null } });
  } finally { finish?.({ status: 'login-required', loginUrl: 'https://login.tailscale.com/a/old', error: null }); await login; }
  expect(client.remote.getState()).toMatchObject({ ok: true, value: { status: 'online', loginUrl: null } });
});
it('ignores helper status callbacks retained across disable and restart epochs', async () => {
  const client = await remoteFixture(new Map<string, FakeRemoteHelper>(), 'client'); cleanups.push(client.dispose);
  const callbacks: Array<(status: HelperStatus) => void> = [];
  vi.spyOn(client.helper, 'subscribeStatus').mockImplementation(listener => { callbacks.push(listener); return () => {}; });
  await client.remote.configure({ enabled: true, hostname: 'client' }); expect(callbacks).toHaveLength(1);
  await client.remote.configure({ enabled: false, hostname: 'client' });
  callbacks[0]({ status: 'login-required', loginUrl: 'https://login.tailscale.com/a/stale', error: null });
  expect(client.remote.getState()).toMatchObject({ ok: true, value: { status: 'disabled', loginUrl: null } });
  await client.remote.configure({ enabled: true, hostname: 'client' });
  callbacks[0]({ status: 'error', loginUrl: null, error: 'stale' });
  expect(client.remote.getState()).toMatchObject({ ok: true, value: { status: 'online', loginUrl: null } });
});
