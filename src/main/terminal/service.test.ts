import { describe, it, expect, vi } from 'vitest';
import { randomUUID } from 'node:crypto';
import type { Result, SessionDto } from '../../shared/contracts';
import { sessionSchema, snapshotSchema } from '../../shared/schemas';
import type { TabObservation } from '../../shared/native-port';
import type { SessionRecord, TabRecord } from '../models';
import { PtyBackend } from './backend';
import { EmbeddedSessionService } from './service';
import { MemoryRepository, factoryFixture } from './test-fixtures';
const value = <T>(r: Result<T>): T => { if (!r.ok) throw new Error(r.error.code); return r.value; };
async function fixture(repository = new MemoryRepository()) {
  const f = factoryFixture(), backend = new PtyBackend(f.options); let emit!: (items: TabObservation[]) => void;
  const tracker = { setWatch: vi.fn(), dispose: vi.fn() };
  const service = new EmbeddedSessionService(repository, backend, callback => { emit = callback; return tracker; }); await service.initialize();
  const create = async (requestId = randomUUID()) => value(await service.createSession({ cwd: '/work', requestId }));
  const observe = (session: SessionDto, agents: number, health: 'healthy' | 'unknown' = 'healthy') => emit(session.tabs.filter(t => t.terminalKind === 'embedded').map(t => ({ sessionId: session.id, tabId: t.id, observedAt: new Date().toISOString(), root: 'alive', health, agents, reason: health === 'unknown' ? 'snapshot incomplete' : null })));
  return { ...f, backend, service, tracker, repository, create, observe };
}
function seedLegacy(repository: MemoryRepository) {
  const now = new Date().toISOString();
  const session: SessionRecord = { id: randomUUID(), title: 'legacy', cwd: '/work', adapterId: 'windows-terminal', shellId: 'pwsh', shellExecutable: 'C:\\pwsh.exe', createdAt: now, updatedAt: now, settledAt: now, error: null, target: null };
  const tab: TabRecord = { id: randomUUID(), sessionId: session.id, title: 'External shell', cwd: '/work', ordinal: 0, createdAt: now, lifecycle: 'open', operationId: randomUUID(), registration: { sessionId: session.id, tabId: '', operationId: '', shell: { pid: 777, startTime: '123456' }, shellExecutable: session.shellExecutable, cwd: '/work', registeredAt: now }, error: null };
  tab.registration!.tabId = tab.id; tab.registration!.operationId = tab.operationId; repository.saveSession(session); repository.saveTab(tab); return { session, tab };
}
describe('embedded saved-task lifecycle', () => {
  it('forwards confirmed exit independently of a failed save and overlays/reconciles closure', async () => {
    const f = await fixture(), s = await f.create(), received: { type: string }[] = []; f.service.subscribeTerminal(e => received.push(e));
    const save = f.repository.saveTab.bind(f.repository), spy = vi.spyOn(f.repository, 'saveTab').mockImplementation(() => { throw new Error('disk full'); });
    f.processes[0].exit(7);
    expect(received.filter(e => e.type === 'exit')).toHaveLength(1); expect(received.filter(e => e.type === 'error')).toHaveLength(1);
    expect(f.repository.tabs(s.id)[0].lifecycle).toBe('open');
    expect(value(f.service.getSnapshot()).sessions[0].tabs[0]).toMatchObject({ lifecycle: 'closed', exitCode: 7 });
    spy.mockImplementation(save); await f.service.dispose(); expect(f.repository.tabs(s.id)[0]).toMatchObject({ lifecycle: 'closed', terminal: { exitCode: 7 } });
  });
  it('merges launch completion with concurrent rename and settlement rather than restoring stale metadata', async () => {
    const repo = new MemoryRepository(), f = factoryFixture(); let finish!: () => void;
    const barrier = new Promise<void>(resolve => { finish = resolve; });
    const backend = new PtyBackend({ ...f.options, validateCwd: async (_p, cwd) => { await barrier; return cwd; } });
    const service = new EmbeddedSessionService(repo, backend, () => ({ setWatch: () => {}, dispose: () => {} })); await service.initialize();
    const creating = service.createSession({ cwd: '/work', requestId: randomUUID() }); await new Promise(resolve => setImmediate(resolve));
    const saved = repo.sessions()[0]; expect(service.ownedTerminalCount()).toBe(1);
    value(await service.renameSession({ sessionId: saved.id, title: 'Renamed while opening' })); value(await service.settleSession({ sessionId: saved.id, confirmActive: true }));
    const pending = value(await service.activateSession({ sessionId: saved.id })); expect(pending.tabs).toHaveLength(1); expect(f.factory).not.toHaveBeenCalled();
    finish(); const completed = value(await creating); expect(completed.title).toBe('Renamed while opening'); expect(completed.settledAt).not.toBeNull(); expect(completed.tabs).toHaveLength(1); await service.dispose();
  });
  it('failed shutdown keeps shell ownership and permits a later successful retry', async () => {
    const f = await fixture(), s = await f.create(); f.processes[0].kill.mockImplementation(() => { throw new Error('denied'); });
    await expect(f.service.dispose()).rejects.toThrow(); expect(f.service.ownedTerminalCount()).toBe(1); expect(f.repository.tabs(s.id)[0].lifecycle).toBe('open');
    value(f.service.writeTerminal({ tabId: s.tabs[0].id, generation: s.tabs[0].generation!, data: 'exit\r' }));
    f.processes[0].kill.mockImplementation(() => f.processes[0].exit(0)); await f.service.dispose(); expect(f.repository.tabs(s.id)[0].lifecycle).toBe('closed');
  });
  it('creates durable intent, shell and DTO without native window targets', async () => {
    const f = await fixture(), s = await f.create(); expect(s).toMatchObject({ adapterId: 'embedded-pty', terminalLifetime: 'app-owned', shellSurvival: false, canAddTab: true, canFocus: true });
    expect(s.tabs[0]).toMatchObject({ lifecycle: 'open', terminalKind: 'embedded', generation: expect.any(String), profileId: 'login-shell' });
    expect(sessionSchema.safeParse(s).success).toBe(true); expect(snapshotSchema.safeParse(value(f.service.getSnapshot())).success).toBe(true);
    expect(f.repository.tab(s.tabs[0].id)?.registration).toBeNull(); expect(f.tracker.setWatch).toHaveBeenLastCalledWith([expect.objectContaining({ pid: 123, environment: 'local' })], expect.any(Array)); await f.service.dispose();
  });
  it('deduplicates concurrent and persisted create requests without relaunching commands', async () => {
    const f = await fixture(), requestId = randomUUID(); const [a, b] = await Promise.all([f.create(requestId), f.create(requestId)]); expect(a.id).toBe(b.id); expect(f.factory).toHaveBeenCalledTimes(1);
    await f.service.dispose(); const restored = await fixture(f.repository); const c = await restored.create(requestId); expect(c.id).toBe(a.id); expect(restored.factory).not.toHaveBeenCalled(); expect(c.tabs[0].lifecycle).toBe('closed'); await restored.service.dispose();
  });
  it('activates a live session without spawning and coalesces reopen clicks', async () => {
    const f = await fixture(), s = await f.create(); await Promise.all([f.service.activateSession({ sessionId: s.id }), f.service.activateSession({ sessionId: s.id })]); expect(f.factory).toHaveBeenCalledTimes(1);
    f.processes[0].exit(0); const [a, b] = await Promise.all([f.service.activateSession({ sessionId: s.id }), f.service.activateSession({ sessionId: s.id })]);
    expect(value(a).tabs).toHaveLength(2); expect(value(b).id).toBe(s.id); expect(f.factory).toHaveBeenCalledTimes(2); await f.service.dispose();
  });
  it('adds independent shells without HWNDs and supports a WSL tab profile', async () => {
    const f = await fixture(), s = await f.create(); const added = value(await f.service.addTab({ sessionId: s.id, profileId: 'wsl:Ubuntu', cwd: '/home/user', title: 'Guest' }));
    expect(added.tabs).toHaveLength(2); expect(added.tabs[1]).toMatchObject({ profileId: 'wsl:Ubuntu', cwd: '/home/user', title: 'Guest' }); expect(f.backend.live()).toHaveLength(2);
    expect(f.repository.session(s.id)?.target).toBeNull(); await f.service.dispose();
  });
  it('keeps per-tab agent evidence when another tab opens/closes and never reuses it on reopen', async () => {
    const f = await fixture(), s = await f.create(); f.observe(s, 1);
    const added = value(await f.service.addTab({ sessionId: s.id }));
    expect(added.tabs.map(t => t.status)).toEqual(['running', 'unknown']);
    f.observe(added, 2);
    const closed = value(await f.service.closeTab({ tabId: added.tabs[1].id, generation: added.tabs[1].generation! }));
    expect(closed.tabs[0].status).toBe('running');
    expect(closed.tabs[1].lifecycle).toBe('closed');
    expect(closed.tabs[0].agents).toBe(2);
    value(await f.service.closeTab({ tabId: added.tabs[0].id, generation: added.tabs[0].generation! }));
    const retried = value(await f.service.retryTab({ tabId: added.tabs[1].id, confirmPossibleDuplicate: false }));
    expect(retried.tabs).toHaveLength(3);
    expect(retried.tabs[2]).toMatchObject({ status: 'unknown', agents: 0 });
    expect(retried.tabs[2].generation).not.toBe(added.tabs[1].generation);
    expect(retried.tabs[0].agents).toBe(0);
    await f.service.dispose();
  });
  it('does not close terminals on attach, view switches or history settling', async () => {
    const f = await fixture(), s = await f.create(); f.service.attachTerminal({ tabId: s.tabs[0].id }); f.observe(s, 1);
    expect(await f.service.settleSession({ sessionId: s.id, confirmActive: false })).toMatchObject({ ok: false, error: { code: 'SETTLE_CONFIRM_REQUIRED' } });
    const settled = value(await f.service.settleSession({ sessionId: s.id, confirmActive: true })); expect(settled.status).toBe('settled'); expect(f.backend.live()).toHaveLength(1); expect(f.processes[0].kill).not.toHaveBeenCalled();
    expect(value(f.service.getSnapshot()).sessions).toHaveLength(0); f.observe(settled, 3); expect(value(f.service.getHistory({ search: '', status: 'all', page: 1, pageSize: 20 })).items[0].counts.agents).toBe(3);
    expect(await f.service.addTab({ sessionId: s.id })).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } }); await f.service.dispose();
  });
  it('preserves settled state when reopening the same saved task', async () => {
    const f = await fixture(), s = await f.create(); const settled = value(await f.service.settleSession({ sessionId: s.id, confirmActive: true })); f.processes[0].exit(0);
    const reopened = value(await f.service.activateSession({ sessionId: s.id })); expect(reopened.settledAt).toBe(settled.settledAt); expect(reopened.id).toBe(s.id); await f.service.dispose();
  });
  it('preserves running/waiting/unknown counts without pretending a prompt is agent completion', async () => {
    const f = await fixture(), s = await f.create(); expect(value(f.service.getSnapshot()).sessions[0].status).toBe('unknown');
    f.observe(s, 2); expect(value(f.service.getSnapshot()).sessions[0]).toMatchObject({ status: 'running', counts: { agents: 2 } });
    f.observe(s, 0); expect(value(f.service.getSnapshot()).sessions[0].status).toBe('waiting');
    f.observe(s, 0, 'unknown'); expect(value(f.service.getSnapshot()).sessions[0].status).toBe('unknown'); await f.service.dispose();
  });
  it('explicit close terminates exactly the owned tab and checks generation', async () => {
    const f = await fixture(), s = await f.create(), added = value(await f.service.addTab({ sessionId: s.id })), t = added.tabs[0];
    expect(await f.service.closeTab({ tabId: t.id, generation: randomUUID() })).toMatchObject({ ok: false, error: { code: 'TARGET_LOST' } });
    const closed = value(await f.service.closeTab({ tabId: t.id, generation: t.generation! })); expect(closed.tabs[0].lifecycle).toBe('closed'); expect(closed.tabs[1].lifecycle).toBe('open');
    expect(f.processes[0].kill).toHaveBeenCalledTimes(1); expect(f.processes[1].kill).not.toHaveBeenCalled(); await f.service.closeTab({ tabId: t.id, generation: t.generation! }); expect(f.processes[0].kill).toHaveBeenCalledTimes(1); await f.service.dispose();
  });
  it('does not mark a pending OS close as exited or reopen it early', async () => {
    const f = await fixture(), s = await f.create(), t = s.tabs[0]; f.processes[0].kill.mockImplementation(() => {});
    const closing = f.service.closeTab({ tabId: t.id, generation: t.generation! });
    await new Promise(resolve => setImmediate(resolve)); expect(f.repository.tab(t.id)?.lifecycle).toBe('open');
    const activate = f.service.activateSession({ sessionId: s.id }); f.processes[0].exit(0); value(await closing); await activate;
    expect(f.factory).toHaveBeenCalledTimes(2); await f.service.dispose();
  });
  it('archives active legacy-only metadata once at startup without touching processes or embedded tasks', async () => {
    const repo = new MemoryRepository(), legacy = seedLegacy(repo);
    legacy.session.settledAt = null; repo.saveSession(legacy.session);
    const tagged: SessionRecord = { ...legacy.session, id: randomUUID(), adapterId: 'embedded-pty' }; repo.saveSession(tagged);
    repo.saveTab({ ...legacy.tab, id: randomUUID(), sessionId: tagged.id });
    const empty = { ...tagged, id: randomUUID() }; repo.saveSession(empty);
    const embedded = { ...tagged, id: randomUUID() }; repo.saveSession(embedded);
    repo.saveTab({ ...legacy.tab, id: randomUUID(), sessionId: embedded.id, terminal: { kind: 'embedded', profileId: 'login-shell', exitCode: null } });
    const f = await fixture(repo);
    expect(value(f.service.getSnapshot()).sessions.map(s => s.id)).toEqual([embedded.id]);
    const history = value(f.service.getHistory({ search: '', status: 'all', page: 1, pageSize: 20 }));
    expect(history.total).toBe(3);
    const archived = repo.session(legacy.session.id)!;
    expect(archived.settledAt).not.toBeNull();
    expect(repo.tab(legacy.tab.id)).toEqual(legacy.tab);
    expect(f.factory).not.toHaveBeenCalled(); expect(f.tracker.setWatch).toHaveBeenCalledWith([], expect.anything());
    expect(await f.service.unsettleSession({ sessionId: legacy.session.id })).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
    await f.service.dispose();
    const restarted = await fixture(repo);
    expect(repo.session(legacy.session.id)?.settledAt).toBe(archived.settledAt);
    expect(value(restarted.service.getSnapshot()).sessions.map(s => s.id)).toEqual([embedded.id]);
    expect(restarted.factory).not.toHaveBeenCalled(); await restarted.service.dispose();
  });
  it('retains legacy history and roots without adopting, watching or killing them', async () => {
    const repo = new MemoryRepository(), legacy = seedLegacy(repo), f = await fixture(repo);
    expect(f.backend.live()).toHaveLength(0); expect(f.tracker.setWatch).toHaveBeenCalledWith([], expect.anything());
    const history = value(f.service.getHistory({ search: 'legacy', status: 'all', page: 1, pageSize: 20 })); expect(history.items[0].tabs[0]).toMatchObject({ terminalKind: 'external-legacy', lifecycle: 'open', agents: 0, status: 'unknown' });
    expect(f.service.attachTerminal({ tabId: legacy.tab.id })).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } }); expect(await f.service.closeTab({ tabId: legacy.tab.id, generation: legacy.tab.operationId })).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
    const active = value(await f.service.activateSession({ sessionId: legacy.session.id })); expect(active.id).toBe(legacy.session.id); expect(active.tabs).toHaveLength(2); expect(active.settledAt).toBe(legacy.session.settledAt);
    await f.service.dispose(); expect(repo.tab(legacy.tab.id)?.registration?.shell.pid).toBe(777); expect(repo.tab(legacy.tab.id)?.lifecycle).toBe('open');
  });
  it('never persists output or exposes it in snapshots', async () => {
    const f = await fixture(), s = await f.create(); f.processes[0].output('secret-terminal-output');
    expect(JSON.stringify(value(f.service.getSnapshot()))).not.toContain('secret-terminal-output'); expect(JSON.stringify([...f.repository.tabMap.values()])).not.toContain('secret-terminal-output');
    expect(value(f.service.attachTerminal({ tabId: s.tabs[0].id })).chunks[0].data).toBe('secret-terminal-output'); await f.service.dispose();
  });
  it('restores app-owned tabs as closed without auto-launch and opens a fresh generation on activation', async () => {
    const f = await fixture(), s = await f.create(); const repo = f.repository;
    // Simulate restart metadata while the previous process is absent, not shell restoration.
    const next = await fixture(repo); expect(next.factory).not.toHaveBeenCalled(); expect(repo.tabs(s.id)[0].lifecycle).toBe('closed');
    const opened = value(await next.service.activateSession({ sessionId: s.id })); expect(opened.tabs[1].generation).not.toBe(s.tabs[0].generation); await next.service.dispose(); await f.service.dispose();
  });
  it('clears old rule observations and rejects arbitrary profile/executable settings', async () => {
    const f = await fixture(), s = await f.create(); f.observe(s, 1); const settings = f.repository.settings();
    expect(await f.service.saveSettings({ ...settings, terminalProfileId: 'malicious' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
    expect(await f.service.saveSettings({ ...settings, shellExecutable: '/bin/arbitrary' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
    value(await f.service.saveSettings({ ...settings, processRules: [] })); expect(value(f.service.getSnapshot()).sessions[0].status).toBe('unknown'); await f.service.dispose();
  });
  it('validates requests and reports spawn failures as durable errors', async () => {
    const f = await fixture(); expect(await f.service.createSession({ cwd: 'relative', requestId: randomUUID() })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
    vi.mocked(f.factory).mockImplementation(() => { throw new Error('spawn'); }); const failed = await f.create(); expect(failed.tabs[0]).toMatchObject({ lifecycle: 'closed', error: { code: 'LAUNCH_FAILED' } }); await f.service.dispose();
  });
});
