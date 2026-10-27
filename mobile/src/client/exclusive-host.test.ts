import { randomUUID } from 'node:crypto';
import { afterEach, expect, it, vi } from 'vitest';
import { z } from 'zod';
import { responseSchemas } from '../../../src/shared/schemas';
import { FakeRemoteHelper, remoteFixture, signedHeaders } from '../../../src/main/remote/test-fixtures';
import { TerminalBridge } from '../terminal/bridge';
import type { HostMessage } from '../terminal/protocol';
import { RemoteClient } from './client';
const cleanups: Array<() => void | Promise<void>> = [];
afterEach(async () => { for (const cleanup of cleanups.splice(0).reverse()) await cleanup(); });
it('gives a mobile-created terminal priority over an eager host desktop view', async () => {
  const host = await remoteFixture(new Map<string, FakeRemoteHelper>(), 'host'); cleanups.push(host.dispose);
  const desktop = host.service.createTerminalView(() => {});
  const attempts: unknown[] = [];
  const unsubscribe = host.service.subscribe(() => {
    const snapshot = host.service.getSnapshot();
    if (snapshot.ok) for (const session of snapshot.value.sessions) for (const tab of session.tabs) {
      if (tab.lifecycle === 'open') attempts.push(desktop.attach({ tabId: tab.id }));
    }
  });
  cleanups.push(() => { unsubscribe(); desktop.dispose(); });
  const client = new RemoteClient('host', { request: async (_peer, route, body) => {
    const response = await fetch(host.helper.backendUrl + route, { method: 'POST', body,
      headers: signedHeaders(host.helper.secret, { id: 'mobile-creator', hostname: 'mobile' }, route, body) });
    return response.text();
  } }, { createClientId: randomUUID });
  cleanups.push(() => client.stop());
  client.connect();
  await vi.waitFor(() => expect(host.remote.getState()).toMatchObject({ ok: true, value: { pending: [expect.objectContaining({ id: 'mobile-creator' })] } }));
  await host.remote.approve({ peerId: 'mobile-creator' });
  await vi.waitFor(() => expect(client.getState().status).toBe('connected'), { timeout: 3000 });
  const created = await client.createSession({ cwd: '/work', requestId: randomUUID() });
  if (!created.ok) throw new Error(created.error.message);
  expect(attempts).not.toHaveLength(0);
  for (const attempt of attempts) expect(attempt).toMatchObject({ ok: true, value: { control: 'blocked' } });
  const tab = created.value.tabs[0], states = vi.fn();
  const bridge = new TerminalBridge({ api: client.terminalApi, subscribeTerminal: client.subscribeTerminal,
    tabId: tab.id, viewId: 'created', fontSize: 14, onState: states, onError: vi.fn(), send: () => {} });
  cleanups.push(() => bridge.dispose());
  bridge.start();
  await vi.waitFor(() => expect(states).toHaveBeenLastCalledWith('attached'));
  bridge.input('creator input');
  await vi.waitFor(() => expect(host.processes[0].write).toHaveBeenCalledWith('creator input'));
  expect(desktop.take({ tabId: tab.id, generation: tab.generation! })).toEqual({ ok: true, value: { taken: true } });
  await vi.waitFor(() => expect(states).toHaveBeenLastCalledWith('blocked'));
});
it('keeps a second mobile viewer blank, transfers with Take it, and promotes the remaining viewer when the owner backgrounds', async () => {
  const host = await remoteFixture(new Map<string, FakeRemoteHelper>(), 'host'); cleanups.push(host.dispose);
  const created = await host.create(); if (!created.ok) throw new Error('Create failed');
  const tab = created.value.tabs[0]; if (!tab.generation) throw new Error('Missing terminal generation');
  const methodSchema = z.object({ method: z.string() });
  function mobile(peerId: string) {
    const messages: HostMessage[] = [], onState = vi.fn(), onError = vi.fn(), epochs: number[] = [];
    const client = new RemoteClient('host', { request: async (_peer, route, body) => {
      const response = await fetch(host.helper.backendUrl + route, { method: 'POST', body,
        headers: signedHeaders(host.helper.secret, { id: peerId, hostname: peerId }, route, body) });
      const json = await response.text();
      if (route === '/v1/request' && methodSchema.parse(JSON.parse(body)).method === 'attachTerminal') {
        const attached = responseSchemas.attachTerminal.parse(JSON.parse(json));
        if (attached.ok && attached.value.control === 'owned') epochs.push(attached.value.controlEpoch);
      }
      return json;
    } }, { createClientId: randomUUID });
    const bridge = new TerminalBridge({ api: client.terminalApi, subscribeTerminal: client.subscribeTerminal,
      tabId: tab.id, viewId: peerId, fontSize: 14, onState, onError, send: message => messages.push(message) });
    cleanups.push(() => { bridge.dispose(); client.stop(); });
    const latest = () => messages.filter(message => message.type === 'output').at(-1);
    const rendered = () => { const output = latest(); if (output) bridge.receive({ type: 'rendered', viewId: peerId, epoch: output.epoch, ticket: output.ticket }); };
    client.connect(); return { client, bridge, messages, onState, onError, epochs, latest, rendered };
  }
  const a = mobile('mobile-a'), b = mobile('mobile-b');
  await vi.waitFor(() => expect(host.remote.getState()).toMatchObject({ ok: true, value: { pending: expect.arrayContaining([expect.objectContaining({ id: 'mobile-a' }), expect.objectContaining({ id: 'mobile-b' })]) } }));
  await host.remote.approve({ peerId: 'mobile-a' }); await host.remote.approve({ peerId: 'mobile-b' });
  await vi.waitFor(() => expect([a.client.getState().status, b.client.getState().status]).toEqual(['connected', 'connected']), { timeout: 3000 });
  a.bridge.start(); await vi.waitFor(() => expect(a.onState).toHaveBeenLastCalledWith('attached')); a.rendered();
  b.bridge.start(); await vi.waitFor(() => expect(b.onState).toHaveBeenLastCalledWith('blocked'));
  expect(b.latest()).toBeUndefined(); expect(b.messages.at(-1)).toMatchObject({ type: 'clear' });
  host.processes[0].output('private owner contents');
  await vi.waitFor(() => expect(a.latest()?.data).toContain('private owner contents')); a.rendered();
  expect(b.latest()).toBeUndefined();
  await b.bridge.takeControl();
  await vi.waitFor(() => expect(a.onState).toHaveBeenLastCalledWith('blocked'));
  await vi.waitFor(() => expect(b.onState).toHaveBeenLastCalledWith('attached'));
  expect(a.messages.at(-1)).toMatchObject({ type: 'clear' }); expect(b.latest()?.data).toContain('private owner contents');
  a.bridge.input('must not write'); b.bridge.input('owner input');
  await vi.waitFor(() => expect(host.processes[0].write).toHaveBeenCalledWith('owner input'));
  expect(host.processes[0].write).not.toHaveBeenCalledWith('must not write');
  b.client.suspend(); b.bridge.dispose();
  await vi.waitFor(() => expect(a.onState).toHaveBeenLastCalledWith('attached'));
  expect(a.epochs.at(-1)).not.toBe(a.epochs[0]);
  expect(await a.client.resizeTerminal({ tabId: tab.id, generation: tab.generation, controlEpoch: a.epochs[0], cols: 52, rows: 24 }))
    .toMatchObject({ ok: false, error: { code: 'TERMINAL_CONTROL_REQUIRED' } });
  expect(a.onError).not.toHaveBeenCalled(); expect(b.onError).not.toHaveBeenCalled();
});
