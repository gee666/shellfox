import { randomUUID } from 'node:crypto';
import { describe, expect, it, vi } from 'vitest';
import type { Result } from '../../shared/contracts';
import { PtyBackend } from './backend';
import { EmbeddedSessionService } from './service';
import { factoryFixture, MemoryRepository } from './test-fixtures';
const value = <T>(r: Result<T>): T => { if (!r.ok) throw new Error(r.error.message); return r.value; };
async function fixture() {
  const f = factoryFixture(), repository = new MemoryRepository(), backend = new PtyBackend(f.options);
  const service = new EmbeddedSessionService(repository, backend, () => ({ setWatch() {}, dispose() {} }));
  await service.initialize();
  const session = value(await service.createSession({ cwd: '/work', title: 'Keep metadata', requestId: randomUUID() }));
  return { ...f, repository, backend, service, session };
}
describe('database retention lifecycle', () => {
  it.each(['exit', 'close'] as const)('%s removes the tab and every retry/request record but keeps its session and sibling', async action => {
    const f = await fixture(), tab = f.session.tabs[0];
    try {
      const added = value(await f.service.addTab({ sessionId: f.session.id }));
      const operation = f.repository.operation(tab.generation!)!;
      f.repository.saveOperation({ ...operation, id: randomUUID(), kind: 'retry', requestId: randomUUID() });
      if (action === 'exit') f.processes[0].exit(7);
      else value(await f.service.closeTab({ tabId: tab.id, generation: tab.generation! }));
      expect(f.repository.tab(tab.id)).toBeUndefined(); expect(f.repository.operationsForTab(tab.id)).toEqual([]);
      expect(f.repository.sessionByRequest(operation.requestId!)).toBeUndefined();
      expect(f.repository.tabs(f.session.id).map(t => t.id)).toEqual([added.tabs[1].id]);
      expect(f.repository.session(f.session.id)?.title).toBe('Keep metadata');
    } finally { await f.service.dispose(); }
  });
  it.each(['exit', 'delete'] as const)('denies new attachments after %s while the existing owner can acknowledge final output', async action => {
    const f = await fixture(), tab = f.session.tabs[0];
    const owner = f.service.createTerminalView(() => {}), other = f.service.createTerminalView(() => {});
    try {
      const attachment = value(owner.attach({ tabId: tab.id }));
      if (attachment.control !== 'owned') throw new Error('Missing test owner');
      f.processes[0].output('retained final screen');
      if (action === 'exit') f.processes[0].exit(0);
      else value(await f.service.deleteSession({ sessionId: f.session.id }));
      expect(f.repository.tab(tab.id)).toBeUndefined();
      expect(value(f.backend.attach({ tabId: tab.id })).chunks.map(chunk => chunk.data).join('')).toContain('retained final screen');
      expect(owner.acknowledge({ tabId: tab.id, generation: tab.generation!, controlEpoch: attachment.controlEpoch, sequence: 1 })).toEqual({ ok: true, value: { acknowledged: true } });
      expect(owner.attach({ tabId: tab.id })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
      expect(owner.take({ tabId: tab.id, generation: tab.generation! })).toMatchObject({ ok: false, error: { code: 'TARGET_LOST' } });
      expect(other.attach({ tabId: tab.id })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
    } finally { owner.dispose(); other.dispose(); await f.service.dispose(); }
  });
  it('settles and unsets without closing terminals or removing saved ownership', async () => {
    const f = await fixture();
    try {
      value(await f.service.setSessionEnv({ sessionId: f.session.id, env: [{ name: 'CUSTOM', value: 'retained' }] }));
      value(await f.service.setSessionPinned({ sessionId: f.session.id, pinned: true }));
      value(await f.service.addTab({ sessionId: f.session.id }));
      const tabs = f.repository.tabs(f.session.id), operations = f.repository.operationMap.size;
      const settled = value(await f.service.settleSession({ sessionId: f.session.id, confirmActive: false }));
      expect(settled).toMatchObject({ id: f.session.id, status: 'settled', env: [{ name: 'CUSTOM', value: 'retained' }], pinnedAt: expect.any(String) });
      expect(settled.tabs.map(t => t.id)).toEqual(tabs.map(t => t.id));
      expect(f.backend.live()).toHaveLength(2); expect(f.repository.operationMap.size).toBe(operations);
      expect(f.processes.every(p => p.kill.mock.calls.length === 0)).toBe(true);
      expect(value(f.service.getSnapshot()).sessions.map(s => s.id)).toContain(f.session.id);
      expect(value(await f.service.unsettleSession({ sessionId: f.session.id })).tabs.map(t => t.id)).toEqual(tabs.map(t => t.id));
    } finally { await f.service.dispose(); }
  });
  it('partial deletion failure keeps the session and still-running tabs owned', async () => {
    const f = await fixture();
    try {
      const added = value(await f.service.addTab({ sessionId: f.session.id }));
      f.processes[1].kill.mockImplementation(() => { throw new Error('denied'); });
      expect(await f.service.deleteSession({ sessionId: f.session.id })).toMatchObject({ ok: false });
      expect(f.repository.session(f.session.id)?.settledAt).toBeNull();
      expect(f.repository.tabs(f.session.id).map(t => t.id)).toEqual([added.tabs[1].id]);
      expect(f.backend.live().map(t => t.tabId)).toEqual([added.tabs[1].id]);
      f.processes[1].kill.mockImplementation(() => f.processes[1].exit(0));
      expect(value(await f.service.deleteSession({ sessionId: f.session.id }))).toEqual({ deleted: true });
    } finally { f.processes[1].kill.mockImplementation(() => f.processes[1].exit(0)); await f.service.dispose(); }
  });
  it('a failed settlement save preserves all terminal metadata and runtime ownership', async () => {
    const f = await fixture(), before = f.repository.tabs(f.session.id);
    const save = vi.spyOn(f.repository, 'saveSession').mockImplementation(() => { throw new Error('disk full'); });
    try {
      expect(await f.service.settleSession({ sessionId: f.session.id, confirmActive: false })).toMatchObject({ ok: false, error: { code: 'STORAGE_FAILED' } });
      expect(f.repository.session(f.session.id)?.settledAt).toBeNull(); expect(f.repository.tabs(f.session.id)).toEqual(before);
      expect(f.backend.live()).toHaveLength(1); expect(f.processes[0].kill).not.toHaveBeenCalled();
      save.mockRestore();
      expect(value(await f.service.settleSession({ sessionId: f.session.id, confirmActive: false })).tabs).toHaveLength(1);
    } finally { save.mockRestore(); await f.service.dispose(); }
  });
  it('an immediate exit during launch cannot recreate the removed tab or operation', async () => {
    const f = factoryFixture(), repository = new MemoryRepository();
    const backend = new PtyBackend({ ...f.options, factory: (...args) => {
      const pty = f.factory(...args);
      return Object.assign(pty, { whenReady: async () => { f.processes.at(-1)!.exit(0); } });
    } });
    const service = new EmbeddedSessionService(repository, backend, () => ({ setWatch() {}, dispose() {} }));
    await service.initialize();
    try {
      expect(await service.createSession({ cwd: '/work', requestId: randomUUID() })).toMatchObject({ ok: false, error: { code: 'TARGET_LOST' } });
      // The action failed, but its retained session must not recreate the exited shell.
      const session = value(service.getSnapshot()).sessions[0];
      expect(session.tabs).toEqual([]); expect(repository.operationMap.size).toBe(0); expect(repository.session(session.id)).toBeDefined();
    } finally { await service.dispose(); }
  });
});
