import { describe, it, expect, vi } from 'vitest';
import { randomUUID } from 'node:crypto';
import type { Result } from '../../shared/contracts';
import type { ControlRequest } from '../control-types';
import { failure } from '../../shared/contracts';
import { PtyBackend } from './backend';
import { EmbeddedSessionService } from './service';
import { MemoryRepository, factoryFixture } from './test-fixtures';

const value = <T>(r: Result<T>): T => { if (!r.ok) throw new Error(r.error.message); return r.value; };
async function fixture() {
  const f = factoryFixture(), repository = new MemoryRepository();
  const backend = new PtyBackend(f.options);
  const service = new EmbeddedSessionService(repository, backend, () => ({ setWatch() {}, dispose() {} }));
  await service.initialize();
  const a = value(await service.createSession({ cwd: '/a', requestId: randomUUID() }));
  const b = value(await service.createSession({ cwd: '/b', requestId: randomUUID() }));
  const marker = backend.get(a.tabs[0]!.id)!.root.marker;
  return { ...f, repository, backend, service, a, b, marker };
}

describe('serialized CLI session control', () => {
  it('resolves exact IDs, unique prefixes and live markers, never a saved title or PID', async () => {
    const { service, a, marker } = await fixture();
    try {
      for (const target of [{ id: a.id }, { id: a.id.slice(0, 8) }, { marker }]) expect(value(await service.controlSession({ action: 'inspect', ...target })).session?.id).toBe(a.id);
      expect(await service.controlSession({ action: 'inspect', id: randomUUID() })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
      expect(await service.controlSession({ action: 'inspect', marker: randomUUID() })).toMatchObject({ ok: false, error: { code: 'TARGET_LOST' } });
      expect(await service.controlSession({ action: 'inspect', id: '/a' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
    } finally { await service.dispose(); }
  });
  it('reports ambiguous prefixes instead of picking one session', async () => {
    const { service, repository, a } = await fixture();
    const other = { ...repository.session(a.id)!, id: a.id.slice(0, -1) + (a.id.endsWith('0') ? '1' : '0') };
    repository.saveSession(other);
    try {
      expect(await service.controlSession({ action: 'inspect', id: a.id.slice(0, 8) })).toMatchObject({ ok: false, error: { code: 'TARGET_AMBIGUOUS' } });
      expect(value(await service.controlSession({ action: 'inspect', id: a.id })).session?.id).toBe(a.id);
    } finally { await service.dispose(); }
  });
  it('rejects unconfirmed archive/delete without closing anything', async () => {
    const { service, a, processes, repository } = await fixture();
    try {
      for (const action of ['archive', 'delete'] as const) for (const confirmed of [undefined, false]) expect(await service.controlSession({ action, id: a.id, confirmed })).toMatchObject({ ok: false, error: { code: 'SETTLE_CONFIRM_REQUIRED' } });
      expect(processes[0]!.kill).not.toHaveBeenCalled(); expect(repository.session(a.id)?.settledAt).toBeNull();
    } finally { await service.dispose(); }
  });
  it.each(['archive', 'delete'] as const)('%s closes every target tab before changing history, without nested queue deadlocks', async action => {
    const { service, backend, a, b, marker, repository, processes } = await fixture();
    value(await service.addTab({ sessionId: a.id }));
    const close = backend.close.bind(backend);
    const spy = vi.spyOn(backend, 'close').mockImplementation(async input => {
      expect(repository.session(a.id)?.settledAt).toBeNull();
      return close(input);
    });
    try {
      value(await service.controlSession({ action, marker, confirmed: true }));
      expect(spy).toHaveBeenCalledTimes(2);
      expect(processes[0]!.kill).toHaveBeenCalledTimes(1); expect(processes[2]!.kill).toHaveBeenCalledTimes(1);
      expect(processes[1]!.kill).not.toHaveBeenCalled(); expect(backend.live().map(t => t.sessionId)).toEqual([b.id]);
      if (action === 'delete') expect(repository.session(a.id)).toBeUndefined();
      else { expect(repository.session(a.id)?.settledAt).toBeTruthy(); expect(value(service.getSnapshot()).sessions.map(s => s.id)).toEqual([b.id]); }
    } finally { spy.mockRestore(); await service.dispose(); }
  });
  it('does not archive or delete if any closure is unconfirmed', async () => {
    const { service, backend, repository, a } = await fixture();
    const spy = vi.spyOn(backend, 'close').mockResolvedValue(failure('TARGET_LOST', 'Close unconfirmed.'));
    try {
      expect(await service.controlSession({ action: 'delete', id: a.id, confirmed: true })).toMatchObject({ ok: false });
      expect(repository.session(a.id)?.settledAt).toBeNull(); expect(repository.tabs(a.id)).toHaveLength(1);
    } finally { spy.mockRestore(); await service.dispose(); }
  });
  it('tab operations require context and close only the invoking generation', async () => {
    const { service, backend, a, b, marker, processes } = await fixture();
    try {
      expect(await service.controlSession({ action: 'tab-open', id: a.id })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
      expect(await service.controlSession({ action: 'tab-close', marker, id: b.id })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
      const added = value(await service.controlSession({ action: 'tab-open', marker })).session!;
      expect(added.tabs).toHaveLength(2);
      value(await service.controlSession({ action: 'tab-close', marker }));
      expect(processes[0]!.kill).toHaveBeenCalledTimes(1); expect(processes[1]!.kill).not.toHaveBeenCalled(); expect(processes[2]!.kill).not.toHaveBeenCalled();
      expect(backend.live().filter(t => t.sessionId === a.id)).toHaveLength(1);
      expect(await service.controlSession({ action: 'tab-open', marker })).toMatchObject({ ok: false, error: { code: 'TARGET_LOST' } });
    } finally { await service.dispose(); }
  });
  it('rechecks context after waiting behind a renderer close in the same session queue', async () => {
    const { service, a, marker, repository, processes } = await fixture();
    try {
      const close = service.closeTab({ tabId: a.tabs[0]!.id, generation: a.tabs[0]!.generation! });
      const archive = service.controlSession({ action: 'archive', marker, confirmed: true });
      value(await close);
      expect(await archive).toMatchObject({ ok: false, error: { code: 'TARGET_LOST' } });
      expect(repository.session(a.id)?.settledAt).toBeNull(); expect(processes).toHaveLength(2);
    } finally { await service.dispose(); }
  });
  it('rejects saved markers for closed roots even if descendant cleanup remains', async () => {
    const { service, backend, a, marker } = await fixture();
    const live = backend.live.bind(backend);
    const spy = vi.spyOn(backend, 'live').mockImplementation(() => live().map(t => t.sessionId === a.id ? { ...t, state: 'closed', cleanupPending: true } : t));
    try { expect(await service.controlSession({ action: 'inspect', marker })).toMatchObject({ ok: false, error: { code: 'TARGET_LOST' } }); }
    finally { spy.mockRestore(); await service.dispose(); }
  });
  it('refuses context whose saved tab generation no longer matches the backend', async () => {
    const { service, repository, a, marker } = await fixture();
    const tab = repository.tab(a.tabs[0]!.id)!;
    try {
      repository.saveTab({ ...tab, operationId: randomUUID() });
      expect(await service.controlSession({ action: 'tab-close', marker })).toMatchObject({ ok: false, error: { code: 'TARGET_LOST' } });
    } finally { repository.saveTab(tab); await service.dispose(); }
  });
  it('does not archive when authoritative exits cannot be persisted', async () => {
    const { service, repository, a } = await fixture();
    const remove = repository.deleteTab.bind(repository);
    const spy = vi.spyOn(repository, 'deleteTab').mockImplementation(id => {
      if (repository.tab(id)?.sessionId === a.id) throw new Error('Storage unavailable.');
      return remove(id);
    });
    try {
      expect(await service.controlSession({ action: 'archive', id: a.id, confirmed: true })).toMatchObject({ ok: false, error: { code: 'STORAGE_FAILED' } });
      expect(repository.session(a.id)?.settledAt).toBeNull();
    } finally { spy.mockRestore(); await service.dispose(); }
  });
  it('rolls back the archive flag if permanent deletion cannot be saved', async () => {
    const { service, a, repository } = await fixture();
    const deleting = vi.spyOn(repository, 'deleteSession').mockReturnValue(false);
    try {
      expect(await service.controlSession({ action: 'delete', id: a.id, confirmed: true })).toMatchObject({ ok: false, error: { code: 'STORAGE_FAILED' } });
      expect(repository.session(a.id)?.settledAt).toBeNull();
      expect(repository.tabs(a.id)).toEqual([]);
    } finally { deleting.mockRestore(); await service.dispose(); }
  });
  it('captures validated request values before queuing, preventing confirmation bypass by mutation', async () => {
    const { service, a, repository, processes } = await fixture();
    try {
      const request: ControlRequest = { action: 'inspect', id: a.id };
      const inspected = service.controlSession(request);
      request.action = 'delete'; request.confirmed = true;
      expect(value(await inspected).session?.id).toBe(a.id);
      expect(repository.session(a.id)?.settledAt).toBeNull(); expect(processes[0]!.kill).not.toHaveBeenCalled();
    } finally { await service.dispose(); }
  });
  it('does not mutate after a disconnected request waits in the queue', async () => {
    const { service, a, repository, processes } = await fixture();
    const controller = new AbortController();
    try {
      const deleting = service.controlSession({ action: 'delete', id: a.id, confirmed: true }, controller.signal);
      controller.abort();
      expect(await deleting).toMatchObject({ ok: false, error: { code: 'TARGET_LOST' } });
      expect(repository.session(a.id)?.settledAt).toBeNull(); expect(processes[0]!.kill).not.toHaveBeenCalled();
    } finally { await service.dispose(); }
  });
  it.each(['archive', 'delete'] as const)('finishes %s atomically if closing the invoking shell aborts its transport', async action => {
    const { service, backend, a, b, marker, repository, processes } = await fixture();
    value(await service.addTab({ sessionId: a.id }));
    const controller = new AbortController(), close = backend.close.bind(backend);
    const spy = vi.spyOn(backend, 'close').mockImplementation(async input => {
      const result = await close(input); controller.abort(); return result;
    });
    try {
      value(await service.controlSession({ action, marker, confirmed: true }, controller.signal));
      expect(spy).toHaveBeenCalledTimes(2);
      expect(backend.live().map(t => t.sessionId)).toEqual([b.id]);
      expect(processes[2]!.kill).toHaveBeenCalled(); expect(processes[1]!.kill).not.toHaveBeenCalled();
      if (action === 'delete') expect(repository.session(a.id)).toBeUndefined();
      else expect(repository.session(a.id)?.settledAt).toBeTruthy();
    } finally { spy.mockRestore(); await service.dispose(); }
  });
  it.each(['archive', 'delete'] as const)('keeps tab creation behind the entire %s operation', async action => {
    const { service, a, repository, processes } = await fixture();
    try {
      const removing = service.controlSession({ action, id: a.id, confirmed: true });
      const adding = service.addTab({ sessionId: a.id });
      value(await removing);
      expect(await adding).toMatchObject({ ok: false, error: { code: action === 'delete' ? 'NOT_FOUND' : 'UNSUPPORTED' } });
      expect(processes).toHaveLength(2);
      if (action === 'delete') expect(repository.session(a.id)).toBeUndefined();
      else expect(repository.session(a.id)?.settledAt).toBeTruthy();
    } finally { await service.dispose(); }
  });
  it('serializes an open before delete so the new tab is also closed', async () => {
    const { service, a, processes, repository } = await fixture();
    try {
      const add = service.addTab({ sessionId: a.id });
      const remove = service.controlSession({ action: 'delete', id: a.id, confirmed: true });
      value(await add); value(await remove);
      expect(processes[2]!.kill).toHaveBeenCalled(); expect(repository.session(a.id)).toBeUndefined();
    } finally { await service.dispose(); }
  });
});
