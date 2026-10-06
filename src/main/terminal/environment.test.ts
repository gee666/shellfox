import { expect, it } from 'vitest';
import { terminalEnvironment } from './environment';
import { EmbeddedSessionService } from './service';
import { PtyBackend } from './backend';
import { factoryFixture, MemoryRepository } from './test-fixtures';
import { randomUUID } from 'node:crypto';
it('treats prototype-like environment names as ordinary names', () => { const env = terminalEnvironment({}, [{ name: '__proto__', value: 'literal' }], { windows: true, wsl: false }); expect(Object.hasOwn(env, '__proto__')).toBe(true); expect(env.__proto__).toBe('literal'); });
it('merges case-insensitively on Windows and keeps WSL names unflagged while preserving unrelated WSLENV', () => {
  expect(terminalEnvironment({ Path: 'C:\\old', API_KEY: 'old', WSLENV: 'API_KEY/p:OTHER/l', SHELLFOX_TEST_MODE: '1' }, [{ name: 'api_key', value: 'new' }, { name: 'MODE', value: 'dev' }], { windows: true, wsl: true, cliBin: 'C:\\Shellfox\\bin' }))
    .toEqual({ TERM: 'xterm-256color', COLORTERM: 'truecolor', Path: 'C:\\Shellfox\\bin;C:\\old', api_key: 'new', MODE: 'dev', WSLENV: 'OTHER/l:TERM:COLORTERM:api_key:MODE' });
});
it.each([false, true])('advertises truecolor instead of inherited host limitations, Windows=%s', windows => {
  expect(terminalEnvironment({ TERM: 'dumb', COLORTERM: '24bit' }, [], { windows, wsl: false })).toMatchObject({ TERM: 'xterm-256color', COLORTERM: 'truecolor' });
  expect(terminalEnvironment({}, [{ name: 'TERM', value: 'vt100' }, { name: 'COLORTERM', value: '' }], { windows, wsl: false })).toMatchObject({ TERM: 'vt100', COLORTERM: '' });
});
it('exports capabilities without path/direction flags and merges session WSLENV without losing unrelated flags', () => {
  const env = terminalEnvironment({ WSLENV: 'OLD/p:TERM/w:COLORTERM/l' }, [
    { name: 'wslenv', value: 'KEEP/up:term/pw:COLORTERM/l:MODE/p' },
    { name: 'term', value: 'vt100' }, { name: 'colorterm', value: '24bit' }, { name: 'MODE', value: 'literal' },
  ], { windows: true, wsl: true });
  expect(env).toEqual({ TERM: 'vt100', COLORTERM: '24bit', MODE: 'literal', WSLENV: 'KEEP/up:TERM:COLORTERM:MODE' });
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
