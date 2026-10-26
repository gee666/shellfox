import { describe, expect, it } from 'vitest';
import { CliUsageError, parseGlobals } from './arguments';
import { parseCommand } from './commands';
import { trailingHelpRequested } from './global-values';
const id = '11111111-1111-4111-8111-111111111111';
describe('global CLI arguments', () => {
  it('accepts globals before and after normal commands', () => {
    expect(parseGlobals(['--host=peer', 'sessions', 'list', '--json', '--user-data', 'a b', '--yes'])).toEqual({
      args: ['sessions', 'list'], options: { host: 'peer', userData: 'a b', json: true, yes: true },
    });
  });
  it('preserves script invocations, required SSH values, and explicit literal arguments', () => {
    expect(parseGlobals(['--json', 'cmd', 'run', '--host', 'literal', '--yes']).args).toEqual(['cmd', 'run', '--host', 'literal', '--yes']);
    expect(parseGlobals(['ssh', 'work', '--host=literal']).options.host).toBe('literal');
    expect(parseGlobals(['ssh', '--profile', '--host=literal']).options.host).toBeUndefined();
    expect(parseGlobals(['terminals', 'write', id, '--', '--json']).args).toEqual(['terminals', 'write', id, '--', '--json']);
  });
  it.each(['commands', 'cmd'])('accepts trailing globals for known %s administration, but not inside script argv', namespace => {
    expect(parseGlobals([namespace, '--list', '--json'])).toMatchObject({ args: [namespace, '--list'], options: { json: true } });
    expect(parseGlobals([namespace, '--register', 'name', 'script', '--json'])).toMatchObject({ args: [namespace, '--register', 'name', 'script'], options: { json: true } });
    expect(parseGlobals([namespace, '--register', 'name', '--json'])).toMatchObject({ args: [namespace, '--register', 'name', '--json'], options: { json: false } });
    expect(parseGlobals([namespace, '--rename', 'old', '--json', '--json'])).toMatchObject({ args: [namespace, '--rename', 'old', '--json'], options: { json: true } });
    expect(parseGlobals([namespace, '--complete', '--json'])).toMatchObject({ args: [namespace, '--complete', '--json'], options: { json: false } });
    for (const operation of ['--remove', '--rm', '--unregister']) expect(parseGlobals([namespace, operation, 'name', '--yes', '--json'])).toMatchObject({ args: [namespace, operation, 'name'], options: { json: true, yes: true } });
    expect(parseGlobals([namespace, '--run', '--json', 'myscript'])).toMatchObject({ args: [namespace, '--run', 'myscript'], options: { json: true } });
    for (const args of [[namespace, 'myscript', '--json'], [namespace, '--run', 'myscript', '--json']]) expect(parseGlobals(args)).toMatchObject({ args, options: { json: false } });
  });
  it('does not steal global-looking strings from context-specific option values', () => {
    expect(parseGlobals(['session', 'create', '--title', '--json'])).toMatchObject({ args: ['session', 'create', '--title', '--json'], options: { json: false } });
    expect(parseCommand(parseGlobals(['session', 'create', '--title', '--json']).args)).toMatchObject({ title: '--json' });
    expect(parseGlobals(['session', 'list', '--name', '--yes', '--json'])).toMatchObject({ args: ['session', 'list', '--name', '--yes'], options: { json: true, yes: false } });
    expect(parseGlobals(['session', 'rename', '--json', 'title'])).toMatchObject({ args: ['session', 'rename', 'title'], options: { json: true } });
    expect(parseGlobals(['ssh', 'profile', '--json'])).toMatchObject({ args: ['ssh', 'profile'], options: { json: true } });
    expect(parseGlobals(['ssh', '--profile', '--json'])).toMatchObject({ args: ['ssh', '--profile', '--json'], options: { json: false } });
  });
  it('does not turn values, script arguments, or text after -- into help requests', () => {
    for (const args of [['session', 'create', '--title', '--help'], ['session', 'rename', '--', '--help'], ['session', 'rename', 'help'], ['commands', 'script', '--help'], ['cmd', '--register', 'name', '--help']]) expect(trailingHelpRequested(args)).toBe(false);
    for (const args of [['session', '--help'], ['session', 'rename', '--help'], ['commands', '--help'], ['commands', '--list', '--help']]) expect(trailingHelpRequested(args)).toBe(true);
  });
  it.each([['--json=false'], ['--json', '--json'], ['--host'], ['--host', '--json'], ['--user-data='], ['--yes=true']])('rejects ambiguous or missing globals %j', (...args) => {
    expect(() => parseGlobals(args)).toThrow(CliUsageError);
  });
});
describe('host command parser', () => {
  it.each(['run', 'status', 'stop'])('parses host %s', operation => expect(parseCommand(['host', operation])).toEqual({ kind: 'host', operation }));
  it('validates the generic manager channel and refuses unrelated methods', () => {
    expect(parseCommand(['api', 'renameSession', '--input', JSON.stringify({ sessionId: id, title: 'renamed' })])).toMatchObject({ kind: 'request', method: 'renameSession', destructive: false });
    expect(() => parseCommand(['api', 'deleteSession', '--input', JSON.stringify({ sessionId: id, extra: true })])).toThrow(CliUsageError);
    expect(() => parseCommand(['api', '__proto__'])).toThrow(CliUsageError);
    expect(() => parseCommand(['api', 'unknown'])).toThrow(CliUsageError);
    expect(() => parseCommand(['api', 'getSnapshot', '--input', '{'])).toThrow(CliUsageError);
  });
  it('marks destructive methods, including generic calls', () => {
    expect(parseCommand(['session', 'delete', id])).toMatchObject({ kind: 'session', operation: 'delete', sessionId: id });
    expect(parseCommand(['session', 'settle', id, '--confirm-active'])).toMatchObject({ kind: 'session', payload: { confirmActive: true } });
    expect(parseCommand(['api', 'installUpdate', '--input', '{"confirmCloseTerminals":true}'])).toMatchObject({ destructive: true });
    expect(parseCommand(['session', 'settle', id])).toMatchObject({ payload: { confirmActive: false } });
  });
  it('rejects secret argv and accepts only stdin or a protected file', () => {
    expect(parseCommand(['network', 'login', '--auth-key-stdin'])).toMatchObject({ kind: 'remote', authKeyStdin: true });
    expect(parseCommand(['network', 'login', '--auth-key-file', 'private.key'])).toMatchObject({ authKeyFile: 'private.key' });
    for (const args of [['network', 'login', '--auth-key', 'secret'], ['network', 'login', '--auth-key-file', 'file', '--auth-key-stdin'], ['network', 'status', '--auth-key-stdin'], ['remote', 'api', 'login', '--input', '{"authKey":"secret"}']]) expect(() => parseCommand(args)).toThrow(CliUsageError);
  });
  it.each([
    [['session', 'list'], 'control'], [['ss', 'inspect', id], 'session'], [['sessions', 'create', '.'], 'create-session'],
    [['session', 'rename', id, 'name'], 'session'], [['session', 'pin', id, '--off'], 'session'],
    [['session', 'env', id, '--input', '[{"name":"LANG","value":"C"}]'], 'session'],
    [['terminals', 'list'], 'view'], [['tm', 'create', id, '--profile', 'bash'], 'terminal-session'],
    [['terminals', 'rename', id, id, 'title'], 'terminal-session'], [['terminals', 'reorder', id, id], 'terminal-session'],
    [['terminals', 'attach', id], 'terminal'], [['terminals', 'write', id, '--stdin'], 'terminal'],
    [['settings', 'read'], 'view'], [['settings', 'update', '--input', '{"historyPageSize":30}'], 'settings-update'],
    [['profiles', 'list'], 'request'], [['path', 'resolve', '~'], 'request'],
    [['machines', 'connect', 'peer-1'], 'remote'], [['approvals', 'reject', 'peer-1'], 'remote'],
    [['remote', 'api', 'getState'], 'remote-api'],
  ])('parses %j', (args, kind) => expect(parseCommand(args)).toMatchObject({ kind }));
  it('does not resolve remote directory paths on the client OS', () => {
    expect(parseCommand(['sessions', 'create', '/remote/project'], true)).toMatchObject({ cwd: '/remote/project' });
    expect(parseCommand(['sessions', 'create'], true)).toMatchObject({ cwd: '~' });
  });
  it.each([
    ['sessions', 'create', '--request-id', 'bad'], ['sessions', 'inspect', 'short'],
    ['sessions', 'delete', id, '--off'], ['sessions', 'list', '--title', 'ignored'],
    ['terminals', 'write', id], ['terminals', 'write', id, 'data', '--stdin'],
    ['terminals', 'attach', id, '--stdin'], ['settings', 'read', '--input', '{}'],
    ['host', 'stop', 'extra'], ['approvals', 'approve', '../bad'],
  ])('rejects invalid or ignored arguments %j', (...args) => expect(() => parseCommand(args)).toThrow(CliUsageError));
  it.each(['ls', 'list', 'status', 'proc', 'ps', 'rm', 'delete', 'tab', 'keepalive', 'ssh', 'cmd', 'start'])('does not accept removed top-level operation %s as a manager command', command => {
    expect(parseCommand([command])).toBeNull();
  });
});
