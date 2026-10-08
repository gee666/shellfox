import { randomUUID } from 'node:crypto';
import { describe, expect, it, vi } from 'vitest';
import { UpdateShutdown } from './update-shutdown';
import { EmbeddedSessionService } from '../terminal/service';
import { PtyBackend } from '../terminal/backend';
import { MemoryRepository, factoryFixture } from '../terminal/test-fixtures';
function fixture() {
  let committed = false;
  const resume = vi.fn(), closeTerminals = vi.fn(async () => resume), confirm = vi.fn(() => true), finish = vi.fn(async () => {});
  const handoff = { get committed() { return committed; }, commit: vi.fn(async () => { committed = true; }), cancel: vi.fn(async () => {}) };
  const start = vi.fn(async () => handoff);
  const shutdown = new UpdateShutdown({ confirm, closeTerminals, finish });
  return { shutdown, resume, closeTerminals, confirm, finish, handoff, start };
}
describe('update shutdown transaction', () => {
  it('does nothing on native cancellation, and confirms helper readiness before closing shells', async () => {
    const f = fixture(); f.confirm.mockReturnValueOnce(false);
    expect(await f.shutdown.run(f.start)).toBe(false); expect(f.start).not.toHaveBeenCalled(); expect(f.closeTerminals).not.toHaveBeenCalled();
    expect(await f.shutdown.run(f.start)).toBe(true);
    expect(f.start.mock.invocationCallOrder[0]).toBeLessThan(f.closeTerminals.mock.invocationCallOrder[0]!);
    expect(f.closeTerminals.mock.invocationCallOrder[0]).toBeLessThan(f.handoff.commit.mock.invocationCallOrder[0]!);
    expect(f.handoff.commit.mock.invocationCallOrder[0]).toBeLessThan(f.finish.mock.invocationCallOrder[0]!);
    expect(f.resume).not.toHaveBeenCalled();
    expect(await f.shutdown.run(f.start)).toBe(false); expect(f.start).toHaveBeenCalledTimes(1);
  });
  it('helper startup failure leaves terminals untouched and allows retry', async () => {
    const f = fixture(); f.start.mockRejectedValueOnce(new Error('script startup failed'));
    await expect(f.shutdown.run(f.start)).rejects.toThrow('startup failed'); expect(f.closeTerminals).not.toHaveBeenCalled();
    expect(await f.shutdown.run(f.start)).toBe(true);
  });
  it('cancels the helper on terminal-close failure and retries without an armed installer', async () => {
    const f = fixture(); f.closeTerminals.mockRejectedValueOnce(new Error('kill denied'));
    await expect(f.shutdown.run(f.start)).rejects.toThrow('kill denied');
    expect(f.handoff.cancel).toHaveBeenCalledTimes(1); expect(f.handoff.commit).not.toHaveBeenCalled(); expect(f.finish).not.toHaveBeenCalled();
    expect(await f.shutdown.run(f.start)).toBe(true);
  });
  it('resumes only after failed handoff cancellation is confirmed, including a later normal quit', async () => {
    const f = fixture(); f.handoff.commit.mockRejectedValueOnce(new Error('commit failed')); f.handoff.cancel.mockRejectedValueOnce(new Error('cancel failed'));
    await expect(f.shutdown.run(f.start)).rejects.toThrow('cancel failed'); expect(f.resume).not.toHaveBeenCalled(); expect(f.finish).not.toHaveBeenCalled();
    await f.shutdown.cancelPending(); expect(f.resume).toHaveBeenCalledTimes(1);
    expect(await f.shutdown.run(f.start)).toBe(true);
  });
  it('blocks concurrent attempts and never retries after acknowledged commit even when final cleanup fails', async () => {
    const f = fixture(); let ready!: () => void;
    f.closeTerminals.mockImplementationOnce(() => new Promise(resolve => { ready = () => resolve(f.resume); }));
    const pending = f.shutdown.run(f.start); await expect.poll(() => typeof ready).toBe('function');
    expect(await f.shutdown.run(f.start)).toBe(false); ready();
    f.finish.mockRejectedValueOnce(new Error('repository close')); await expect(pending).rejects.toThrow();
    expect(f.handoff.cancel).not.toHaveBeenCalled(); expect(f.resume).not.toHaveBeenCalled(); expect(await f.shutdown.run(f.start)).toBe(false);
  });
});
async function serviceFixture() {
  const f = factoryFixture(), repo = new MemoryRepository(), tracker = { setWatch: vi.fn(), dispose: vi.fn() };
  const backend = new PtyBackend(f.options), service = new EmbeddedSessionService(repo, backend, () => tracker); await service.initialize();
  const create = () => service.createSession({ cwd: '/work', requestId: randomUUID() });
  await create(); return { ...f, repo, tracker, backend, service, create };
}
describe('real embedded-service recovery', () => {
  it('can open fresh shells after commit failure, then successfully update or normally quit', async () => {
    const f = await serviceFixture(), handoff = { committed: false, commit: vi.fn().mockRejectedValueOnce(new Error('helper died')).mockResolvedValueOnce(undefined), cancel: vi.fn(async () => {}) };
    const shutdown = new UpdateShutdown({ confirm: () => true, closeTerminals: () => f.service.closeForUpdate(), finish: () => f.service.dispose() });
    await expect(shutdown.run(async () => handoff)).rejects.toThrow('helper died');
    expect(f.service.ownedTerminalCount()).toBe(0); expect(f.tracker.dispose).not.toHaveBeenCalled();
    expect(await f.create()).toMatchObject({ ok: true, value: { tabs: [expect.objectContaining({ lifecycle: 'open' })] } });
    expect(await shutdown.run(async () => handoff)).toBe(true); expect(f.service.ownedTerminalCount()).toBe(0); expect(f.tracker.dispose).toHaveBeenCalledTimes(1);
  });
  it('exit persistence failure during normal shutdown does not irreversibly dispose the backend', async () => {
    const f = await serviceFixture(); const save = f.repo.saveTab.bind(f.repo), spy = vi.spyOn(f.repo, 'saveTab').mockImplementation(() => { throw new Error('disk full'); });
    await expect(f.service.dispose()).rejects.toThrow('metadata remains unsaved');
    expect(f.tracker.dispose).not.toHaveBeenCalled(); spy.mockImplementation(save);
    expect(await f.create()).toMatchObject({ ok: true }); expect(f.service.ownedTerminalCount()).toBe(1);
    await f.service.dispose(); expect(f.service.ownedTerminalCount()).toBe(0);
  });
  it('blocks new shells during reversible shutdown and restores service after partial termination failure', async () => {
    const f = await serviceFixture(); f.processes[0]!.kill.mockImplementation(() => { throw new Error('denied'); });
    const closing = f.service.closeForUpdate(); expect(await f.create()).toMatchObject({ ok: false });
    await expect(closing).rejects.toThrow('not confirmed');
    expect(await f.create()).toMatchObject({ ok: true });
    f.processes[0]!.kill.mockImplementation(() => f.processes[0]!.exit(0));
    const resume = await f.service.closeForUpdate(); expect(await f.create()).toMatchObject({ ok: false }); resume();
    expect(await f.create()).toMatchObject({ ok: true }); await f.service.dispose();
  });
  it('drains a shell already launching before acknowledging reversible terminal closure', async () => {
    const f = factoryFixture(), repo = new MemoryRepository(); let spawn!: () => void;
    const barrier = new Promise<void>(resolve => { spawn = resolve; });
    const backend = new PtyBackend({ ...f.options, validateCwd: async (_profile, cwd) => { await barrier; return cwd; } });
    const service = new EmbeddedSessionService(repo, backend, () => ({ setWatch: vi.fn(), dispose: vi.fn() })); await service.initialize();
    const creating = service.createSession({ cwd: '/work', requestId: randomUUID() });
    await expect.poll(() => service.ownedTerminalCount()).toBe(1);
    let closed = false; const shuttingDown = service.closeForUpdate().then(resume => { closed = true; return resume; });
    expect(closed).toBe(false); spawn(); expect(await creating).toMatchObject({ ok: true });
    const resume = await shuttingDown; expect(service.ownedTerminalCount()).toBe(0); expect(f.processes[0]!.kill).toHaveBeenCalled();
    resume(); expect(await service.createSession({ cwd: '/work', requestId: randomUUID() })).toMatchObject({ ok: true }); await service.dispose();
  });
});
