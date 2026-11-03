import { afterEach, expect, it, vi } from 'vitest';
import type { Result, SaveSettingsInput } from '../shared/contracts';
import { SessionService } from './service';
import { UnsupportedBackend } from './platform';
import { EmbeddedSessionService } from './terminal/service';
import { PtyBackend } from './terminal/backend';
import { factoryFixture, MemoryRepository } from './terminal/test-fixtures';

const value = <T>(result: Result<T>): T => { if (!result.ok) throw new Error(result.error.message); return result.value; };
const cleanup: Array<() => Promise<void>> = [];
afterEach(async () => { for (const dispose of cleanup.splice(0)) await dispose(); });
async function fixture(kind: 'legacy' | 'embedded') {
  const repository = new MemoryRepository();
  const f = factoryFixture(), legacyBackend = new UnsupportedBackend();
  vi.spyOn(legacyBackend, 'setWatch').mockResolvedValue({ ok: true, value: { configured: true } });
  const service = kind === 'embedded'
    ? new EmbeddedSessionService(repository, new PtyBackend(f.options), () => ({ setWatch() {}, dispose() {} }))
    : new SessionService(repository, legacyBackend);
  await service.initialize({ userDataDir: '/work', helperDir: '/work', shellScriptDir: '/work', packagedExecutable: null });
  cleanup.push(() => service.dispose());
  return { repository, service };
}

it.each(['legacy', 'embedded'] as const)('%s setTerminalEnv preserves unrelated settings, canonicalizes input and emits settings', async kind => {
  const { repository, service } = await fixture(kind);
  const previous = repository.settings(), changes: string[] = [];
  service.subscribe(event => changes.push(event.reason));
  const env = [{ name: ' GLOBAL ', value: 'literal $() ; 雪' }, { name: 'EMPTY', value: '' }];
  const pending = service.setTerminalEnv({ env });
  env[0].value = 'mutated after validation';
  const saved = value(await pending);
  expect(saved).toEqual({ ...previous, terminalEnv: [{ name: 'GLOBAL', value: 'literal $() ; 雪' }, { name: 'EMPTY', value: '' }] });
  expect(repository.settings()).toEqual(saved);
  expect(value(service.getSnapshot()).settings).toEqual(saved);
  expect(changes).toEqual(['settings']);
  expect(value(await service.setTerminalEnv({ env: [] })).terminalEnv).toEqual([]);
});

it.each(['legacy', 'embedded'] as const)('%s setTerminalEnv validates using the host case policy and reports persistence failure', async kind => {
  const { repository, service } = await fixture(kind);
  const changes: string[] = [];
  service.subscribe(event => changes.push(event.reason));
  expect(await service.setTerminalEnv({ env: [{ name: 'BAD=NAME', value: '' }] })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  const caseDistinct = await service.setTerminalEnv({ env: [{ name: 'MODE', value: 'upper' }, { name: 'mode', value: 'lower' }] });
  expect(caseDistinct.ok).toBe(process.platform !== 'win32');
  const previous = repository.settings();
  changes.length = 0;
  vi.spyOn(repository, 'saveSettings').mockImplementation(() => { throw new Error('disk full'); });
  expect(await service.setTerminalEnv({ env: [{ name: 'VALID', value: 'unsaved' }] })).toMatchObject({ ok: false, error: { code: 'STORAGE_FAILED', retryable: true } });
  expect(repository.settings()).toEqual(previous);
  expect(changes).toEqual([]);
});

it.each(['legacy', 'embedded'] as const)('%s setTerminalEnv rechecks remote permission inside the settings queue without blocking local writes', async kind => {
  const { repository, service } = await fixture(kind);
  const changes: string[] = [];
  service.subscribe(event => changes.push(event.reason));
  const localEnv = [{ name: 'LOCAL', value: 'allowed' }];
  const local = service.setTerminalEnv({ env: localEnv });
  let approved = true;
  const canCommit = vi.fn(() => approved);
  const remote = service.setTerminalEnv({ env: [{ name: 'REMOTE', value: 'revoked' }] }, canCommit);
  expect(canCommit).not.toHaveBeenCalled();
  approved = false;
  expect(await local).toMatchObject({ ok: true });
  expect(await remote).toMatchObject({ ok: false, error: { code: 'ACCESS_DENIED' } });
  expect(canCommit).toHaveBeenCalledOnce();
  expect(repository.settings().terminalEnv).toEqual(localEnv);
  expect(changes).toEqual(['settings']);
  approved = true;
  expect(await service.setTerminalEnv({ env: [] }, canCommit)).toMatchObject({ ok: true });
  expect(repository.settings().terminalEnv).toEqual([]);
});

it.each(['legacy', 'embedded'] as const)('%s setTerminalEnv shares the settings queue and merges into the latest settings', async kind => {
  const { repository, service } = await fixture(kind);
  const candidate = { ...repository.settings(), accentColor: '#123456', autoUpdate: false };
  const env = [{ name: 'GLOBAL', value: 'queued' }];
  const results = await Promise.all([service.saveSettings(candidate), service.setTerminalEnv({ env })]);
  results.forEach(value);
  expect(repository.settings()).toMatchObject({ accentColor: '#123456', autoUpdate: false, terminalEnv: env });
});

it.each(['legacy', 'embedded'] as const)('%s omitted terminalEnv preserves a newer queued global-env edit', async kind => {
  const { repository, service } = await fixture(kind);
  const staleInput: SaveSettingsInput = { ...repository.settings(), accentColor: '#654321' };
  delete staleInput.terminalEnv;
  const env = [{ name: 'GLOBAL', value: 'newer than the settings draft' }];
  // The settings input predates this global edit, but commits after it.
  const results = await Promise.all([service.setTerminalEnv({ env }), service.saveSettings(staleInput)]);
  results.forEach(value);
  expect(value(results[1]).terminalEnv).toEqual(env);
  expect(repository.settings()).toMatchObject({ accentColor: '#654321', terminalEnv: env });
});

it.each(['legacy', 'embedded'] as const)('%s explicit terminalEnv still replaces or clears global values', async kind => {
  const { repository, service } = await fixture(kind);
  const stale = repository.settings();
  value(await service.setTerminalEnv({ env: [{ name: 'GLOBAL', value: 'new' }] }));
  const explicit = [{ name: 'OVERRIDE', value: 'intentional full-DTO write' }];
  expect(value(await service.saveSettings({ ...stale, terminalEnv: explicit })).terminalEnv).toEqual(explicit);
  expect(repository.settings().terminalEnv).toEqual(explicit);
  expect(value(await service.saveSettings({ ...stale, terminalEnv: [] })).terminalEnv).toEqual([]);
  expect(repository.settings().terminalEnv).toEqual([]);
});
