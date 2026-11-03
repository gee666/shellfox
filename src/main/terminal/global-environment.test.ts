import { randomUUID } from 'node:crypto';
import { afterEach, expect, it, vi } from 'vitest';
import type { EnvVar, Result } from '../../shared/contracts';
import { terminalEnvironment } from './environment';
import { EmbeddedSessionService } from './service';
import { PtyBackend } from './backend';
import { factoryFixture, launchInput, MemoryRepository } from './test-fixtures';

const value = <T>(result: Result<T>): T => { if (!result.ok) throw new Error(result.error.message); return result.value; };
afterEach(() => vi.unstubAllEnvs());

it.each([false, true])('merges inherited < global < session using the host case policy, Windows=%s', windows => {
  const inherited = { TOKEN: 'inherited', token: 'inherited lower', KEEP: 'inherited' };
  const terminalEnv = [{ name: 'TOKEN', value: 'global' }, { name: 'GLOBAL', value: 'literal $() ; 雪' }, { name: '__proto__', value: 'global' }];
  const session = [{ name: 'token', value: '' }, { name: '__proto__', value: 'session' }];
  const env = terminalEnvironment(inherited, session, { windows, wsl: false, terminalEnv });
  expect(env.token).toBe('');
  expect(env.TOKEN).toBe(windows ? undefined : 'global');
  expect(env.GLOBAL).toBe('literal $() ; 雪');
  expect(env.KEEP).toBe('inherited');
  expect(Object.hasOwn(env, '__proto__')).toBe(true);
  expect(env.__proto__).toBe('session');
  expect(inherited.TOKEN).toBe('inherited');
  expect(terminalEnv[0].value).toBe('global');
  expect(session[0].value).toBe('');
});

it.each([false, true])('transfers winning global/session values to direct and nested WSL, direct=%s', wsl => {
  const env = terminalEnvironment({ WSLENV: 'KEEP/p:TOKEN/w', HOST_SECRET: 'private' }, [
    { name: 'token', value: 'session' }, { name: 'SHELLFOX_TERMINAL_MARKER', value: 'not-owned' },
  ], { windows: true, wsl, marker: 'owned', terminalEnv: [
    { name: 'TOKEN', value: 'global' }, { name: 'GLOBAL', value: 'global only' },
    { name: 'WSLENV', value: 'OTHER/l:GLOBAL/pw:TOKEN/u' }, { name: 'term', value: 'vt100' },
  ] });
  expect(env).toMatchObject({ token: 'session', GLOBAL: 'global only', TERM: 'vt100', HOST_SECRET: 'private', SHELLFOX_TERMINAL_MARKER: 'owned' });
  const transfer = env.WSLENV.split(':');
  expect(transfer).toContain('GLOBAL');
  expect(transfer).toContain('token');
  expect(transfer).toContain('OTHER/l');
  expect(transfer).toContain('SHELLFOX_TERMINAL_MARKER');
  expect(transfer.filter(name => name.toLowerCase() === 'token')).toHaveLength(1);
  expect(transfer.some(name => name.startsWith('GLOBAL/') || name === 'HOST_SECRET')).toBe(false);
});

it('applies independent layer limits rather than rejecting 400 valid configured variables', () => {
  const terminalEnv = Array.from({ length: 200 }, (_, i) => ({ name: 'GLOBAL_' + i, value: String(i) }));
  const session = Array.from({ length: 200 }, (_, i) => ({ name: 'SESSION_' + i, value: String(i) }));
  expect(terminalEnvironment({}, session, { windows: false, wsl: false, terminalEnv })).toMatchObject({ GLOBAL_199: '199', SESSION_199: '199' });
});

it('prepends the CLI to Unix PATH without overriding the distinct Path variable', () => {
  const env = terminalEnvironment({ Path: '/case-sensitive' }, [], { windows: false, wsl: false, cliBin: '/cli', terminalEnv: [{ name: 'PATH', value: '/global/bin' }] });
  expect(env.PATH).toBe('/cli:/global/bin');
  expect(env.Path).toBe('/case-sensitive');
});

it.each(['win32', 'linux'] as const)('validates global and session layers independently using the backend platform, %s', async platform => {
  const f = factoryFixture(), backend = new PtyBackend({ ...f.options, platform });
  value(await backend.initialize());
  try {
    const terminalEnv = [{ name: 'MODE', value: 'upper' }, { name: 'mode', value: 'lower' }];
    const result = await backend.launch({ ...launchInput(), terminalEnv });
    expect(result.ok).toBe(platform !== 'win32');
    if (platform === 'linux') {
      expect(vi.mocked(f.factory).mock.calls[0][2].env).toMatchObject({ MODE: 'upper', mode: 'lower' });
    } else expect(f.factory).not.toHaveBeenCalled();
    expect(await backend.launch({ ...launchInput(), terminalEnv: [{ name: 'BAD=NAME', value: '' }] })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
    const valid = await backend.launch({
      ...launchInput(), terminalEnv: [{ name: 'TERM', value: 'global' }, { name: 'GLOBAL', value: 'kept' }],
      env: [{ name: 'TERM', value: 'session' }],
    });
    value(valid);
    expect(vi.mocked(f.factory).mock.calls.at(-1)![2]).toMatchObject({ name: 'session', env: { TERM: 'session', GLOBAL: 'kept' } });
  } finally { await backend.dispose(); }
});

it('applies saved globals to creation, added tabs, CLI tabs, retries and restart activation without changing live shells', async () => {
  vi.stubEnv('GLOBAL_ENV_TEST', 'inherited');
  const repository = new MemoryRepository(), f = factoryFixture();
  let service = new EmbeddedSessionService(repository, new PtyBackend(f.options), () => ({ setWatch() {}, dispose() {} }));
  await service.initialize();
  const spawn = (call: number) => vi.mocked(f.factory).mock.calls[call][2].env;
  try {
    const firstEnv: EnvVar[] = [{ name: 'GLOBAL_ENV_TEST', value: 'global' }, { name: 'GLOBAL_ONLY', value: 'one' }];
    value(await service.setTerminalEnv({ env: firstEnv }));
    const created = value(await service.createSession({ cwd: '/work', requestId: randomUUID() }));
    expect(created.env).toEqual([]);
    expect(spawn(0)).toMatchObject({ GLOBAL_ENV_TEST: 'global', GLOBAL_ONLY: 'one' });
    value(await service.setSessionEnv({ sessionId: created.id, env: [{ name: 'global_env_test', value: 'session' }] }));
    value(await service.setTerminalEnv({ env: [{ name: 'GLOBAL_ENV_TEST', value: 'updated' }, { name: 'GLOBAL_ONLY', value: 'two' }] }));
    expect(f.processes[0].write).not.toHaveBeenCalled();
    expect(spawn(0).GLOBAL_ONLY).toBe('one');
    const added = value(await service.addTab({ sessionId: created.id }));
    expect(added.env).toEqual([{ name: 'global_env_test', value: 'session' }]);
    expect(spawn(1)).toMatchObject({ global_env_test: 'session', GLOBAL_ONLY: 'two' });
    value(await service.controlSession({ action: 'tab-open', marker: service.backend.get(created.tabs[0].id)!.root.marker }));
    expect(spawn(2)).toMatchObject({ global_env_test: 'session', GLOBAL_ONLY: 'two' });
    // Retry uses activation; seed a retained failed-launch tab after the live shells exit.
    const retryTab = repository.tabs(created.id)[0];
    f.processes.forEach(p => p.exit(0));
    repository.saveTab({ ...retryTab, lifecycle: 'closed' });
    value(await service.setTerminalEnv({ env: [] }));
    value(await service.retryTab({ tabId: retryTab.id, confirmPossibleDuplicate: true }));
    expect(spawn(3).global_env_test).toBe('session');
    expect(spawn(3).GLOBAL_ONLY).toBeUndefined();
    value(await service.setTerminalEnv({ env: [{ name: 'GLOBAL_ONLY', value: 'after restart' }] }));
    await service.dispose();
    const second = factoryFixture();
    service = new EmbeddedSessionService(repository, new PtyBackend(second.options), () => ({ setWatch() {}, dispose() {} }));
    await service.initialize();
    expect(second.factory).not.toHaveBeenCalled();
    expect(value(service.getSnapshot()).settings.terminalEnv).toEqual([{ name: 'GLOBAL_ONLY', value: 'after restart' }]);
    value(await service.activateSession({ sessionId: created.id }));
    expect(vi.mocked(second.factory).mock.calls[0][2].env).toMatchObject({ global_env_test: 'session', GLOBAL_ONLY: 'after restart' });
  } finally { await service.dispose(); }
});
