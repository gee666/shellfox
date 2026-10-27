import { expect, it, vi } from 'vitest';
import { randomUUID } from 'node:crypto';
import { PtyBackend } from './backend';
import { EmbeddedSessionService } from './service';
import { factoryFixture, MemoryRepository } from './test-fixtures';

it('does not queue independent session creation behind a slow launch, but still deduplicates request IDs', async () => {
  let release!: () => void;
  const slowDirectory = new Promise<void>(resolve => { release = resolve; });
  const f = factoryFixture(), repository = new MemoryRepository();
  const validateCwd = vi.fn(async (_profile: unknown, cwd: string) => { if (cwd === '/slow') await slowDirectory; return cwd; });
  const service = new EmbeddedSessionService(repository, new PtyBackend({ ...f.options, validateCwd }), () => ({ setWatch() {}, dispose() {} }));
  await service.initialize();
  const requestId = randomUUID();
  const slow = service.createSession({ cwd: '/slow', requestId });
  const duplicate = service.createSession({ cwd: '/slow', requestId });
  await new Promise<void>(resolve => setImmediate(resolve));
  expect(validateCwd).toHaveBeenCalledOnce();
  let fastFinished = false;
  const fast = service.createSession({ cwd: '/fast', requestId: randomUUID() }).then(result => { fastFinished = result.ok; return result; });
  try {
    await new Promise<void>(resolve => setImmediate(resolve));
    expect(fastFinished).toBe(true);
    expect(f.processes).toHaveLength(1);
    expect(repository.sessions()).toHaveLength(2);
  } finally {
    release();
    const [a, b, c] = await Promise.all([slow, duplicate, fast]);
    try {
      expect(a.ok && b.ok && c.ok).toBe(true);
      if (a.ok && b.ok && c.ok) { expect(a.value.id).toBe(b.value.id); expect(c.value.id).not.toBe(a.value.id); }
      expect(f.factory).toHaveBeenCalledTimes(2);
    } finally { await service.dispose(); }
  }
});

it('cancels all independent create queues on shutdown without launching late shells', async () => {
  let release!: () => void;
  const directories = new Promise<void>(resolve => { release = resolve; });
  const f = factoryFixture(), validateCwd = vi.fn(async (_profile: unknown, cwd: string) => { await directories; return cwd; });
  const backend = new PtyBackend({ ...f.options, validateCwd });
  const service = new EmbeddedSessionService(new MemoryRepository(), backend, () => ({ setWatch() {}, dispose() {} }));
  await service.initialize();
  const requestId = randomUUID();
  const requests = [service.createSession({ cwd: '/one', requestId }), service.createSession({ cwd: '/two', requestId: randomUUID() }), service.createSession({ cwd: '/one', requestId })];
  try {
    await new Promise<void>(resolve => setImmediate(resolve));
    expect(validateCwd).toHaveBeenCalledTimes(2);
    await service.dispose();
    const results = await Promise.all(requests);
    // Cancellation reports launch failure even though the saved task remains.
    for (const result of results.slice(0, 2)) expect(result).toMatchObject({ ok: false, error: { code: 'LAUNCH_FAILED' } });
    expect(results[2]).toMatchObject({ ok: false, error: { code: 'NATIVE_UNAVAILABLE' } });
    expect(backend.live()).toHaveLength(0);
  } finally {
    release(); await Promise.all(requests); await service.dispose();
  }
  await new Promise<void>(resolve => setImmediate(resolve));
  expect(f.factory).not.toHaveBeenCalled();
});
