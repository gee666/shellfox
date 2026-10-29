import { randomUUID } from 'node:crypto';
import { describe, expect, it, vi } from 'vitest';
import type { Result } from '../../shared/contracts';
import type { TabObservation } from '../../shared/native-port';
import { responseSchemas } from '../../shared/schemas';
import { managerOperation } from '../../host/operations';
import { remoteOperation } from '../remote/operations';
import { PtyBackend } from './backend';
import { EmbeddedSessionService } from './service';
import { factoryFixture, MemoryRepository } from './test-fixtures';

const value = <T>(result: Result<T>): T => { if (!result.ok) throw new Error(result.error.message); return result.value; };
async function fixture() {
  const f = factoryFixture(), repository = new MemoryRepository(), backend = new PtyBackend(f.options);
  let observe!: (items: TabObservation[]) => void;
  const service = new EmbeddedSessionService(repository, backend, emit => {
    observe = emit;
    return { setWatch() {}, dispose() {} };
  });
  await service.initialize();
  const session = value(await service.createSession({ cwd: '/work', requestId: randomUUID() }));
  const tab = session.tabs[0];
  if (!tab.generation) throw new Error('Missing generation');
  const identity = { tabId: tab.id, generation: tab.generation };
  const settle = () => service.settleSession({ sessionId: session.id, confirmActive: false });
  const current = () => value(service.getSnapshot()).sessions.find(item => item.id === session.id)!;
  return { ...f, repository, backend, service, session, identity, settle, current, observe };
}

describe('settled terminal input', () => {
  it.each(['direct', 'manager', 'remote'] as const)('unsettles immediately through %s input and preserves tabs, pins and environment', async route => {
    const f = await fixture(), view = f.service.createTerminalView(() => {});
    try {
      value(await f.service.addTab({ sessionId: f.session.id }));
      value(await f.service.setSessionPinned({ sessionId: f.session.id, pinned: true }));
      value(await f.service.setSessionEnv({ sessionId: f.session.id, env: [{ name: 'KEEP', value: 'yes' }] }));
      f.observe([{ sessionId: f.session.id, tabId: f.identity.tabId, root: 'alive', health: 'healthy', agents: 1, reason: null, observedAt: new Date().toISOString() }]);
      const settled = value(await f.settle()), tabs = f.repository.tabs(f.session.id);
      const attachment = value(view.attach(f.identity));
      if (attachment.control !== 'owned') throw new Error('Missing owner');
      const input = { ...f.identity, controlEpoch: attachment.controlEpoch, data: 'echo resumed\r' };
      const changed = vi.fn(), unsubscribe = f.service.subscribe(changed);
      const save = vi.spyOn(f.repository, 'saveSession');
      try {
        const write = (payload: typeof input & { userInitiated?: boolean }) => route === 'direct' ? f.service.writeTerminal(payload)
          : route === 'manager' ? managerOperation({ service: f.service, delivery: view, clientId: 'local' }, 'writeTerminal', payload)
          : remoteOperation(f.service, view, 'mobile', 'writeTerminal', payload);
        expect(await write({ ...input, data: '\x1b[1;1R', userInitiated: false })).toEqual({ ok: true, value: { written: true } });
        expect(f.current().settledAt).toBe(settled.settledAt);
        expect(save).not.toHaveBeenCalled();
        expect(changed).not.toHaveBeenCalled();
        expect(responseSchemas.writeTerminal.parse(await write(input))).toEqual({ ok: true, value: { written: true } });
        expect(f.current()).toMatchObject({ settledAt: null, status: 'running', pinnedAt: settled.pinnedAt, env: settled.env });
        expect(f.repository.session(f.session.id)?.settledAt).toBeNull();
        expect(f.repository.tabs(f.session.id)).toEqual(tabs);
        expect(f.backend.live()).toHaveLength(2);
        expect(f.processes[0].write).toHaveBeenCalledTimes(2);
        expect(f.processes[0].write).toHaveBeenLastCalledWith(input.data);
        expect(f.processes.every(process => process.kill.mock.calls.length === 0)).toBe(true);
        expect(save).toHaveBeenCalledOnce();
        expect(changed).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ reason: 'sessions' }));
        value(f.service.writeTerminal({ ...f.identity, data: '\x03' }));
        expect(save).toHaveBeenCalledOnce();
        expect(changed).toHaveBeenCalledOnce();
      } finally { unsubscribe(); }
    } finally { view.dispose(); await f.service.dispose(); }
  });

  it('keeps settlement when viewing, resizing, receiving output or observing processes', async () => {
    const f = await fixture(), view = f.service.createTerminalView(() => {});
    try {
      const settled = value(await f.settle());
      value(f.service.attachTerminal(f.identity));
      const attachment = value(view.attach(f.identity));
      if (attachment.control !== 'owned') throw new Error('Missing owner');
      value(view.resize({ ...f.identity, controlEpoch: attachment.controlEpoch, cols: 100, rows: 30 }));
      value(await f.service.activateSession({ sessionId: f.session.id }));
      f.processes[0].output('background output');
      f.observe([{ sessionId: f.session.id, tabId: f.identity.tabId, root: 'alive', health: 'healthy', agents: 2, reason: null, observedAt: new Date().toISOString() }]);
      expect(f.current()).toMatchObject({ settledAt: settled.settledAt, status: 'settled', activityStatus: 'running' });
    } finally { view.dispose(); await f.service.dispose(); }
  });

  it('does not unsettle for empty, stale, blocked or failed input', async () => {
    const f = await fixture(), owner = f.service.createTerminalView(() => {}), blocked = f.service.createTerminalView(() => {});
    try {
      const settled = value(await f.settle()), attachment = value(owner.attach(f.identity));
      if (attachment.control !== 'owned') throw new Error('Missing owner');
      const input = { ...f.identity, controlEpoch: attachment.controlEpoch, data: 'x' };
      value(blocked.attach(f.identity));
      expect(blocked.write(input)).toMatchObject({ ok: false, error: { code: 'TERMINAL_CONTROL_REQUIRED' } });
      expect(owner.write({ ...input, generation: randomUUID() })).toMatchObject({ ok: false, error: { code: 'TARGET_LOST' } });
      expect(f.service.writeTerminal({ ...f.identity, data: '' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
      f.processes[0].write.mockImplementationOnce(() => { throw new Error('PTY unavailable'); });
      expect(owner.write(input)).toMatchObject({ ok: false, error: { code: 'NATIVE_UNAVAILABLE' } });
      expect(f.current()).toMatchObject({ settledAt: settled.settledAt, status: 'settled' });
      value(owner.write(input));
      expect(f.current().settledAt).toBeNull();
    } finally { owner.dispose(); blocked.dispose(); await f.service.dispose(); }
  });

  it.each(['session', 'saveSession'] as const)('keeps accepted input successful when %s fails and clears settlement on later input', async method => {
    const f = await fixture();
    try {
      value(await f.settle());
      const changed = vi.fn(), unsubscribe = f.service.subscribe(changed);
      const storage = vi.spyOn(f.repository, method).mockImplementationOnce(() => { throw new Error('storage unavailable'); });
      try {
        expect(f.service.writeTerminal({ ...f.identity, data: 'run once\r' })).toEqual({ ok: true, value: { written: true } });
        expect(f.processes[0].write).toHaveBeenCalledExactlyOnceWith('run once\r');
        expect(f.current().status).toBe('settled');
        expect(changed).not.toHaveBeenCalled();
        value(f.service.writeTerminal({ ...f.identity, data: 'next input' }));
        expect(f.current().settledAt).toBeNull();
        expect(changed).toHaveBeenCalledOnce();
      } finally { storage.mockRestore(); unsubscribe(); }
    } finally { await f.service.dispose(); }
  });
});
