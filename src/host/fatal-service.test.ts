import { randomUUID } from 'node:crypto';
import { expect, it, vi } from 'vitest';
import { EmbeddedSessionService } from '../main/terminal/service';
import { PtyBackend } from '../main/terminal/backend';
import { factoryFixture, MemoryRepository } from '../main/terminal/test-fixtures';
import { failure } from '../shared/contracts';

it('never resumes database polling or mutations after fatal terminal cleanup fails', async () => {
  const fixture = factoryFixture(), trackerDisposed = vi.fn();
  const backend = new PtyBackend(fixture.options);
  const service = new EmbeddedSessionService(new MemoryRepository(), backend, () => ({ setWatch() {}, dispose: trackerDisposed }));
  await service.initialize();
  const session = await service.createSession({ cwd: process.cwd(), requestId: randomUUID() }); expect(session.ok).toBe(true);
  const close = vi.spyOn(backend, 'close').mockResolvedValue(failure('NATIVE_UNAVAILABLE', 'Test cleanup refused.'));
  try {
    await expect(service.dispose({ fatal: true })).rejects.toThrow('shutdown was not confirmed');
    expect(trackerDisposed).toHaveBeenCalledOnce();
    expect(await service.createSession({ cwd: process.cwd(), requestId: randomUUID() })).toMatchObject({ ok: false, error: { code: 'NATIVE_UNAVAILABLE' } });
  } finally { close.mockRestore(); await backend.dispose(); }
});
