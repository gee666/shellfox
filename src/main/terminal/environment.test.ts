import { expect, it, vi } from 'vitest';
import { terminalEnvironment } from './environment';
import { EmbeddedSessionService } from './service';
import { PtyBackend } from './backend';
import { factoryFixture, MemoryRepository } from './test-fixtures';
import { randomUUID } from 'node:crypto';
it('treats prototype-like environment names as ordinary names', () => { const env = terminalEnvironment({}, [{ name: '__proto__', value: 'literal' }], { windows: true, wsl: false }); expect(Object.hasOwn(env, '__proto__')).toBe(true); expect(env.__proto__).toBe('literal'); });
it.each([false, true])('merges Windows names and keeps managed WSL names unflagged, direct WSL=%s', wsl => {
  expect(terminalEnvironment({ Path: 'C:\\old', API_KEY: 'old', WSLENV: 'API_KEY/p:OTHER/l', SHELLFOX_TEST_MODE: '1' }, [{ name: 'api_key', value: 'new' }, { name: 'MODE', value: 'dev' }], { windows: true, wsl, cliBin: 'C:\\Shellfox\\bin' }))
    .toEqual({ TERM: 'xterm-256color', COLORTERM: 'truecolor', Path: 'C:\\Shellfox\\bin;C:\\old', api_key: 'new', MODE: 'dev', WSLENV: 'OTHER/l:TERM:COLORTERM:api_key:MODE' });
});
it.each([false, true])('advertises truecolor instead of inherited host limitations, Windows=%s', windows => {
  expect(terminalEnvironment({ TERM: 'dumb', COLORTERM: '24bit' }, [], { windows, wsl: false })).toMatchObject({ TERM: 'xterm-256color', COLORTERM: 'truecolor' });
  expect(terminalEnvironment({}, [{ name: 'TERM', value: 'vt100' }, { name: 'COLORTERM', value: '' }], { windows, wsl: false })).toMatchObject({ TERM: 'vt100', COLORTERM: '' });
});
it.each([false, true])('merges session WSLENV without path/direction flags on managed values, direct WSL=%s', wsl => {
  const env = terminalEnvironment({ WSLENV: 'OLD/p:TERM/w:COLORTERM/l' }, [
    { name: 'wslenv', value: 'KEEP/up:term/pw:COLORTERM/l:MODE/p' },
    { name: 'term', value: 'vt100' }, { name: 'colorterm', value: '24bit' }, { name: 'MODE', value: 'literal' },
  ], { windows: true, wsl });
  expect(env).toEqual({ TERM: 'vt100', COLORTERM: '24bit', MODE: 'literal', WSLENV: 'KEEP/up:TERM:COLORTERM:MODE' });
});
it('prepares nested WSL without a marker and does not opt inherited secrets or the CLI PATH into transfer', () => {
  const inherited = { PRIVATE_TOKEN: 'host secret', Path: 'C:\\host', WSLENV: 'KEEP/ulp' };
  const variables = [{ name: 'TOKEN', value: 'literal ; $() ` " \' 雪 = C:\\foo:bar' }, { name: 'EMPTY', value: '' }];
  const env = terminalEnvironment(inherited, variables, { windows: true, wsl: false, cliBin: 'C:\\Shellfox\\bin' });
  expect(env.WSLENV).toBe('KEEP/ulp:TERM:COLORTERM:TOKEN:EMPTY');
  expect(env.TOKEN).toBe(variables[0].value);
  expect(env.EMPTY).toBe('');
  expect(env.PRIVATE_TOKEN).toBe('host secret');
  expect(inherited).toEqual({ PRIVATE_TOKEN: 'host secret', Path: 'C:\\host', WSLENV: 'KEEP/ulp' });
  expect(variables).toHaveLength(2);
});
it('removes duplicate managed entries and keeps the winning spelling without listing WSLENV itself', () => {
  const env = terminalEnvironment({ WsLeNv: 'MODE/p:mode/w:TERM/l::OTHER/u:OTHER/p', MODE: 'old' }, [
    { name: 'Mode', value: 'new' }, { name: 'COLORTERM', value: '' },
  ], { windows: true, wsl: false });
  expect(env.WSLENV).toBe('OTHER/u:OTHER/p:TERM:COLORTERM:Mode');
  expect(env.WsLeNv).toBeUndefined();
  expect(env.MODE).toBeUndefined();
  expect(env.Mode).toBe('new');
  expect(terminalEnvironment({}, [{ name: 'WSLENV', value: 'OTHER/w' }], { windows: true, wsl: false }).WSLENV)
    .toBe('OTHER/w:TERM:COLORTERM');
});
it.each([false, true])('does not add WSL transport to native Unix shells or conflate case-sensitive names, inherited WSLENV=%s', withWslenv => {
  const env = terminalEnvironment(withWslenv ? { WSLENV: 'KEEP/p:MODE/w' } : {}, [
    { name: 'MODE', value: 'upper' }, { name: 'mode', value: 'lower' },
  ], { windows: false, wsl: false });
  expect(env).toEqual({ TERM: 'xterm-256color', COLORTERM: 'truecolor', MODE: 'upper', mode: 'lower', ...(withWslenv ? { WSLENV: 'KEEP/p:MODE/w' } : {}) });
});
it.each(['pwsh', 'windows-powershell', 'wsl:Ubuntu'])('passes session overrides through backend spawn env for direct or nested WSL from %s', async profileId => {
  const f = factoryFixture();
  const discovered = await f.options.discover();
  const profile = { ...discovered.profiles[profileId === 'wsl:Ubuntu' ? 1 : 0], id: profileId };
  const backend = new PtyBackend({ ...f.options, discover: async () => ({ ...discovered, profiles: [profile], defaultProfileId: profileId }) });
  await backend.initialize();
  try {
    const input = { tabId: randomUUID(), sessionId: randomUUID(), generation: randomUUID(), profileId, cwd: '/work', env: [
      { name: 'SHELLFOX_TEST', value: 'literal $() ; C:\\folder with spaces' },
      { name: 'wslenv', value: 'KEEP/ul:SHELLFOX_TEST/pw:shellfox_terminal_marker/l' },
    ] };
    expect((await backend.launch(input)).ok).toBe(true);
    const [file, args, options] = vi.mocked(f.factory).mock.calls[0];
    const marker = backend.get(input.tabId)!.root.marker;
    expect(options.env.WSLENV).toBe('KEEP/ul:TERM:COLORTERM:SHELLFOX_TEST:SHELLFOX_TERMINAL_MARKER');
    expect(options.env.SHELLFOX_TEST).toBe(input.env[0].value);
    expect(options.env.SHELLFOX_TERMINAL_MARKER).toBe(marker);
    expect(file).toBe(profile.executable);
    expect(args.join(' ')).not.toContain(input.env[0].value);
  } finally { await backend.dispose(); }
});
it('rejects delimiters and multiline overrides before WSL transport', () => { for (const name of ['A:B', 'A/B', 'A B']) expect(() => terminalEnvironment({}, [{ name, value: 'ok' }], { windows: true, wsl: true })).toThrow(); for (const value of ['a\rb','a\nb']) expect(() => terminalEnvironment({}, [{ name: 'GOOD', value }], { windows: true, wsl: true })).toThrow(); });
it('stores canonical env, changes only later tabs/retries, emits sessions and never updates running shells', async () => {
  const f = factoryFixture(), repository = new MemoryRepository(), service = new EmbeddedSessionService(repository, new PtyBackend(f.options), () => ({ setWatch() {}, dispose() {} }));
  await service.initialize();
  try {
    const created = await service.createSession({ cwd: '/work', requestId: randomUUID() }); if (!created.ok) throw new Error('create');
    expect(created.value.env).toEqual([]); const changes: string[] = []; service.subscribe(event => changes.push(event.reason));
    const env = [{ name: ' SHELLFOX_TEST ', value: 'round3' }, { name: 'TERM', value: 'dumb' }], result = await service.setSessionEnv({ sessionId: created.value.id, env });
    if (!result.ok) throw new Error('env');
    expect(result.value.env).toEqual([{ name: 'SHELLFOX_TEST', value: 'round3' }, { name: 'TERM', value: 'dumb' }]); expect(changes.at(-1)).toBe('sessions');
    expect(f.processes[0].write).not.toHaveBeenCalled();
    await service.addTab({ sessionId: created.value.id });
    expect((f.factory as any).mock.calls[0][2].env.SHELLFOX_TEST).toBeUndefined();
    expect((f.factory as any).mock.calls[1][2].env.SHELLFOX_TEST).toBe('round3'); expect((f.factory as any).mock.calls[1][2].env.TERM).toBe('dumb');
    f.processes.forEach(p => p.exit(0));
    await service.retryTab({ tabId: created.value.tabs[0].id, confirmPossibleDuplicate: false });
    expect((f.factory as any).mock.calls[2][2].env.SHELLFOX_TEST).toBe('round3');
    expect((await service.setSessionEnv({ sessionId: created.value.id, env: [{ name: 'A=B', value: '' }] })).ok).toBe(false);
  } finally { await service.dispose(); }
});
