import { randomUUID } from 'node:crypto';
import { describe, expect, it, vi } from 'vitest';
import type { Result } from '../../shared/contracts';
import { requestSchemas } from '../../shared/schemas';
import { EmbeddedSessionService } from './service';
import { PtyBackend } from './backend';
import { factoryFixture, MemoryRepository } from './test-fixtures';
const value = <T>(result: Result<T>): T => { if (!result.ok) throw new Error(result.error.code); return result.value; };
async function fixture(repository = new MemoryRepository()) {
  const f = factoryFixture();
  const service = new EmbeddedSessionService(repository, new PtyBackend(f.options), () => ({ setWatch: () => {}, dispose: () => {} }));
  await service.initialize();
  const session = value(await service.createSession({ cwd: '/work', requestId: randomUUID() }));
  const added = value(await service.addTab({ sessionId: session.id }));
  return { ...f, service, repository, session: added };
}

describe('terminal tab metadata', () => {
  it('renames and reorders without replacing processes, generations, output or active status', async () => {
    const f = await fixture(), [a, b] = f.session.tabs, events = vi.fn(); f.service.subscribe(events);
    f.processes[0].output('retained output');
    const original = f.repository.tabs(f.session.id);
    const renamed = value(await f.service.renameTab({ sessionId: f.session.id, tabId: a.id, title: '  Build 雪  ' }));
    expect(renamed.tabs[0].title).toBe('Build 雪');
    const reordered = value(await f.service.reorderTabs({ sessionId: f.session.id, tabIds: [b.id, a.id] }));
    expect(reordered.tabs.map(tab => [tab.id, tab.ordinal])).toEqual([[b.id, 0], [a.id, 1]]);
    expect(f.repository.tab(a.id)).toEqual({ ...original[0], title: 'Build 雪', ordinal: 1, terminal: { ...original[0].terminal, userTitle: 'Build 雪' } });
    expect(f.repository.tab(b.id)).toEqual({ ...original[1], ordinal: 0 });
    expect(events).toHaveBeenCalledTimes(2);
    expect(value(f.service.attachTerminal({ tabId: a.id })).chunks[0].data).toBe('retained output');
    expect(f.factory).toHaveBeenCalledTimes(2); f.processes.forEach(proc => expect(proc.kill).not.toHaveBeenCalled());
    const next = value(await f.service.addTab({ sessionId: f.session.id })); expect(next.tabs[2].ordinal).toBe(2);
    await f.service.dispose();
  });
  it('rejects invalid, duplicate, incomplete, stale and foreign-session requests without changes', async () => {
    const f = await fixture(), [a, b] = f.session.tabs;
    const other = value(await f.service.createSession({ cwd: '/work', requestId: randomUUID() }));
    const before = f.repository.tabs(), revision = f.service.revision;
    for (const title of ['', '  ', 'x\n', 'x\0', 'x'.repeat(201)]) {
      expect(await f.service.renameTab({ sessionId: f.session.id, tabId: a.id, title })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
    }
    expect(await f.service.renameTab({ sessionId: other.id, tabId: a.id, title: 'foreign' })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
    expect(await f.service.renameTab({ sessionId: f.session.id, tabId: randomUUID(), title: 'missing' })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
    for (const tabIds of [[], [a.id], [a.id, a.id], [a.id, other.tabs[0].id], [a.id, b.id, randomUUID()]]) {
      expect(await f.service.reorderTabs({ sessionId: f.session.id, tabIds })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
    }
    expect(await f.service.reorderTabs({ sessionId: randomUUID(), tabIds: [a.id] })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
    expect(requestSchemas.renameTab.safeParse({ sessionId: f.session.id, tabId: a.id, title: 'ok', executable: '/bad' }).success).toBe(false);
    expect(requestSchemas.reorderTabs.safeParse({ sessionId: f.session.id, tabIds: [a.id, b.id], otherSessionId: other.id }).success).toBe(false);
    expect(f.repository.tabs()).toEqual(before); expect(f.service.revision).toBe(revision);
    await f.service.dispose();
  });
  it('keeps legacy-only tabs read-only', async () => {
    const f = await fixture(), saved = f.repository.session(f.session.id)!;
    const legacy = { ...saved, id: randomUUID(), adapterId: 'windows-terminal' as const };
    const tab = { ...f.repository.tabs(saved.id)[0], id: randomUUID(), sessionId: legacy.id, terminal: null };
    f.repository.saveSession(legacy); f.repository.saveTab(tab);
    expect(await f.service.renameTab({ sessionId: legacy.id, tabId: tab.id, title: 'External' })).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
    expect(await f.service.reorderTabs({ sessionId: legacy.id, tabIds: [tab.id] })).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
    expect(f.repository.tab(tab.id)).toEqual(tab); await f.service.dispose();
  });
  it('rolls back titles, both reorder passes and timestamps on storage failure, emitting no success event', async () => {
    const f = await fixture(), [a, b] = f.session.tabs, before = f.repository.tabs(), sessionBefore = f.repository.session(f.session.id), revision = f.service.revision;
    const sessionSave = vi.spyOn(f.repository, 'saveSession').mockImplementation(() => { throw new Error('disk full'); });
    expect(await f.service.renameTab({ sessionId: f.session.id, tabId: a.id, title: 'unsaved' })).toMatchObject({ ok: false, error: { code: 'STORAGE_FAILED' } });
    expect(f.repository.tabs()).toEqual(before); sessionSave.mockRestore();
    const save = f.repository.saveTab.bind(f.repository); let writes = 0;
    const tabSave = vi.spyOn(f.repository, 'saveTab').mockImplementation(tab => { save(tab); if (++writes === 3) throw new Error('disk full'); });
    expect(await f.service.reorderTabs({ sessionId: f.session.id, tabIds: [b.id, a.id] })).toMatchObject({ ok: false, error: { code: 'STORAGE_FAILED' } });
    expect(f.repository.tabs()).toEqual(before); expect(f.repository.session(f.session.id)).toEqual(sessionBefore); expect(f.service.revision).toBe(revision);
    tabSave.mockRestore(); await f.service.dispose();
  });
  it('keeps tab title and order changes made while a new session launch is pending', async () => {
    const repository = new MemoryRepository(), f = factoryFixture(); let finish!: () => void;
    const barrier = new Promise<void>(resolve => { finish = resolve; });
    const service = new EmbeddedSessionService(repository, new PtyBackend({ ...f.options, validateCwd: async (_profile, cwd) => { await barrier; return cwd; } }), () => ({ setWatch: () => {}, dispose: () => {} }));
    await service.initialize();
    const creating = service.createSession({ cwd: '/work', requestId: randomUUID() });
    await new Promise(resolve => setImmediate(resolve));
    const session = repository.sessions()[0], tab = repository.tabs(session.id)[0];
    const closed = { ...tab, id: randomUUID(), ordinal: 1, lifecycle: 'closed' as const };
    repository.saveTab(closed);
    value(await service.renameTab({ sessionId: session.id, tabId: tab.id, title: 'Starting build' }));
    value(await service.reorderTabs({ sessionId: session.id, tabIds: [closed.id, tab.id] }));
    finish();
    const completed = value(await creating);
    expect(completed.tabs.map(item => [item.id, item.title, item.ordinal, item.lifecycle])).toEqual([[closed.id, closed.title, 0, 'closed'], [tab.id, 'Starting build', 1, 'open']]);
    await service.dispose();
  });
  it('serializes concurrent changes, keeps closed tabs in the permutation and restores metadata after restart', async () => {
    const f = await fixture(), [a, b] = f.session.tabs;
    await Promise.all([
      f.service.renameTab({ sessionId: f.session.id, tabId: a.id, title: 'Build' }),
      f.service.reorderTabs({ sessionId: f.session.id, tabIds: [b.id, a.id] }),
    ]);
    value(await f.service.closeTab({ tabId: a.id, generation: a.generation! }));
    value(await f.service.reorderTabs({ sessionId: f.session.id, tabIds: [a.id, b.id] }));
    value(await f.service.renameTab({ sessionId: f.session.id, tabId: a.id, title: 'Closed build' }));
    await f.service.dispose();
    const backend = new PtyBackend(factoryFixture().options), restarted = new EmbeddedSessionService(f.repository, backend, () => ({ setWatch: () => {}, dispose: () => {} }));
    await restarted.initialize();
    const restored = value(restarted.getSnapshot()).sessions.find(session => session.id === f.session.id)!;
    expect(restored.tabs.map(tab => [tab.id, tab.title, tab.ordinal, tab.lifecycle])).toEqual([[a.id, 'Closed build', 0, 'closed'], [b.id, b.title, 1, 'closed']]);
    expect(backend.live()).toHaveLength(0); await restarted.dispose();
  });
});
