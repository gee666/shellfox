import { expect, it, vi } from 'vitest';
import { PassThrough, Writable } from 'node:stream';
import { confirmControl, filterControl, parseControlArgs, runControlCli } from './control-cli';
import type { ControlRequest, ControlResponse } from './control-types';
import { controlRequestSchema } from './control-protocol';
import type { SessionDto } from '../shared/contracts';
vi.mock('./control-client', () => ({ requestControl: vi.fn() }));
const id = 'c36951c8-03f5-49c7-bb11-ad8828044a31';
const otherId = 'c36951c8-03f5-49c7-bb11-ad8828044a32';
const tabId = 'c36951c8-03f5-49c7-bb11-ad8828044a33';
function session(overrides: Partial<SessionDto> = {}): SessionDto {
  return { id, title: 'Project', cwd: '/work/project', adapterId: 'embedded-pty', shellId: 'bash',
    createdAt: '2026-01-01T00:00:00Z', updatedAt: '2026-01-01T00:00:00Z', settledAt: null,
    status: 'running', activityStatus: 'running', pinnedAt: null, counts: { running: 1, waiting: 0, unknown: 0, error: 0, closed: 0, agents: 0 },
    tabs: [{ id: tabId, sessionId: id, title: 'bash', cwd: '/work/project', ordinal: 0, createdAt: '2026-01-01T00:00:00Z', lifecycle: 'open', status: 'running', agents: 0, monitoringReason: null, error: null }],
    env: [], canFocus: true, canAddTab: true, controlReason: null, error: null, ...overrides };
}
const response: ControlResponse = { sessions: [session(), session({ id: otherId, title: 'Other', cwd: '/other', tabs: [], activityStatus: 'waiting', status: 'waiting' })],
  tabs: [{ tabId, memoryBytes: null, elapsedSeconds: null, processes: [{ pid: 42, parentPid: 1, domain: 'local', name: 'bash', memoryBytes: null, elapsedSeconds: null }] }] };
function output(tty = true) {
  let text = '';
  const stream = Object.assign(new Writable({ write(chunk, _encoding, done) { text += chunk.toString(); done(); } }), { isTTY: tty });
  return { stream, text: () => text };
}
function harness(tty = true, result: ControlResponse = response) {
  const input = Object.assign(new PassThrough(), { isTTY: tty });
  const out = output(tty), err = output(false);
  const request = vi.fn(async (_data: string, req: ControlRequest): Promise<ControlResponse> => {
    controlRequestSchema.parse(req);
    return req.action === 'inspect' ? { session: session(), tabs: response.tabs } : result;
  });
  return { input, out, err, request, options: { input, output: out.stream, error: err.stream, marker: 'c36951c8-03f5-49c7-bb11-ad8828044a34', request } };
}
it.each([
  ['ls', ['ls', 'list', 'sessions']], ['delete', ['delete', 'remove', 'rm']], ['archive', ['archive', 'settle']],
  ['keep-alive', ['keep-alive', 'keepalive', 'awake']], ['stats', ['stats', 'status', 'usage']], ['processes', ['processes', 'proc', 'ps']],
] as const)('parses aliases for %s', (action, aliases) => {
  for (const alias of aliases) expect(parseControlArgs([alias])).toMatchObject({ kind: 'control', action: action === 'ls' ? 'list' : action });
});
it.each(['--version', '-v', 'version'])('parses version %s', alias => expect(parseControlArgs([alias])).toEqual({ kind: 'version' }));
it('supports top-level, subcommand and alias help', () => {
  for (const flag of ['--help', '-h', 'help']) {
    expect(parseControlArgs([flag])).toEqual({ kind: 'help' });
    expect(parseControlArgs(['stats', flag])).toEqual({ kind: 'help', topic: 'stats' });
    expect(parseControlArgs(['tab', 'new', flag])).toEqual({ kind: 'help', topic: 'tab open' });
  }
  expect(parseControlArgs(['help', 'tab', 'rm'])).toEqual({ kind: 'help', topic: 'tab close' });
  for (const alias of ['start', 'add', 'new', 'create']) expect(parseControlArgs([alias, '-h'])).toEqual({ kind: 'help', topic: 'start' });
  expect(parseControlArgs(['help', 'ssh'])).toEqual({ kind: 'help', topic: 'ssh' });
});
it('parses context-only tab aliases and session UUID/current targeting', () => {
  for (const alias of ['open', 'add', 'new']) expect(parseControlArgs(['tab', alias, '--current'])).toEqual({ kind: 'control', action: 'tab-open', filters: { current: true } });
  for (const alias of ['close', 'remove', 'rm']) expect(parseControlArgs(['tab', alias])).toEqual({ kind: 'control', action: 'tab-close', filters: { current: true } });
  expect(parseControlArgs(['delete', '--id=' + id.toUpperCase()])).toMatchObject({ filters: { id } });
  expect(parseControlArgs(['archive'])).toMatchObject({ filters: { current: true } });
});
it('combines read filters and accepts equals syntax', () => {
  expect(parseControlArgs(['ps', '--running', '--no-agents', '--current', '--id', id, '--name=proj', '--path', '/work', '--tab', tabId, '--pid=42'])).toEqual({
    kind: 'control', action: 'processes', filters: { running: true, noAgents: true, current: true, id, name: 'proj', path: '/work', tab: tabId, pid: 42 },
  });
});
it.each([
  ['wat'], ['constructor'], ['toString'], ['__proto__'], ['ls', '--wat'], ['ls', '--running', '--idle'], ['stats', '--pid', '42'], ['ls', '--id', '1'],
  ['ls', '--tab', 'abc'], ['ps', '--pid', '0'], ['ps', '--pid', '1.1'], ['ps', '--pid', '9007199254740992'],
  ['ls', '--name'], ['ls', '--name', ''], ['ls', '--name', 'one', '--name', 'two'], ['ls', '--running=true'],
  ['ls', '--running', '--running'], ['delete', '--yes'], ['delete', '--running'], ['delete', id, '--current'],
  ['delete', id, '--id', id], ['delete', 'Project'], ['tab', 'create'], ['tab', 'open', id], ['tab', 'close', '--id', tabId], ['keep-alive', 'on'], ['version', 'extra'],
  ['help', 'missing'], ['help', 'tab', 'missing'], ['stats', '--name', 'prod', '--help'],
].map(args => ({ args })))('rejects invalid $args', ({ args }) => expect(() => parseControlArgs(args)).toThrow());
it('filters sessions and tab/process rows without mutating the response', () => {
  const copy = JSON.stringify(response);
  const filtered = filterControl(response, { id, running: true, noAgents: true, name: 'PROJ', path: 'WORK', tab: tabId, pid: 42 });
  expect(filtered.sessions).toHaveLength(1); expect(filtered.sessions![0].tabs).toHaveLength(1); expect(filtered.tabs![0].processes).toHaveLength(1);
  expect(filterControl(response, { pid: 99 }).sessions).toHaveLength(0);
  expect(filterControl(response, { id, idle: true }).sessions).toHaveLength(0);
  expect(filterControl(response, { idle: true }).sessions?.map(s => s.id)).toEqual([otherId]);
  expect(filterControl({ sessions: [session({ status: 'settled', activityStatus: 'waiting' }), session({ activityStatus: 'unknown' })] }, { idle: true }).sessions).toEqual([]);
  expect(JSON.stringify(response)).toBe(copy);
});
it('dispatches reads using the marker, with client-side intersection for current + UUID', async () => {
  const h = harness();
  expect(await runControlCli(['ls', '--current', '--id', otherId], '/data', '1.2.3', h.options)).toBe(0);
  expect(h.request).toHaveBeenCalledWith('/data', { action: 'list', current: true, marker: h.options.marker });
  h.request.mockResolvedValue({ sessions: [session()] });
  const out = output();
  expect(await runControlCli(['ls', '--current', '--id', otherId], '/data', '1', { ...h.options, output: out.stream })).toBe(0);
  expect(out.text()).toContain('No matching sessions');
});
it('dispatches tab and keep-alive actions', async () => {
  for (const [args, request] of [
    [['tab', 'open'], { action: 'tab-open', current: true, marker: 'c36951c8-03f5-49c7-bb11-ad8828044a34' }],
    [['tab', 'rm'], { action: 'tab-close', current: true, marker: 'c36951c8-03f5-49c7-bb11-ad8828044a34' }],
    [['awake'], { action: 'keep-alive' }],
  ] as const) {
    const h = harness();
    expect(await runControlCli([...args], '/data', '1', h.options)).toBe(0);
    expect(h.request).toHaveBeenCalledWith('/data', request);
  }
});
it('help/version never contact the running app', async () => {
  const h = harness();
  for (const args of [[], ['-h'], ['help', 'tab', 'close'], ['-v'], ['ssh', '-h']]) {
    expect(await runControlCli(args, '/data', '1.2.3', h.options)).toBe(0);
  }
  expect(h.request).not.toHaveBeenCalled(); expect(h.out.text()).toContain('1.2.3');
});
it('requires a marker for current operations and makes no requests on invalid syntax', async () => {
  const h = harness();
  for (const args of [['delete'], ['ls', '--current'], ['tab', 'close'], ['ls', '--pid', '42']]) {
    expect(await runControlCli(args, '/data', '1', { ...h.options, marker: undefined })).toBe(2);
  }
  expect(h.request).not.toHaveBeenCalled();
});
it.each(['delete', 'archive'])('inspects %s, prompts, then pins the confirmed UUID', async action => {
  const h = harness();
  const pending = runControlCli([action], '/data', '1', h.options);
  await vi.waitFor(() => expect(h.out.text()).toContain('[y/N]'));
  expect(h.request).toHaveBeenCalledTimes(1);
  expect(h.request).toHaveBeenLastCalledWith('/data', { action: 'inspect', marker: h.options.marker, current: true });
  expect(h.out.text()).toContain('Project'); expect(h.out.text()).toContain('PID 42');
  h.input.write('YES\n');
  expect(await pending).toBe(0);
  expect(h.request).toHaveBeenLastCalledWith('/data', { action, id, confirmed: true });
});
it.each(['n\n', '\n', 'yep\n', 'true\n'])('cancels destructive operations on answer %j', async answer => {
  const h = harness();
  const pending = runControlCli(['rm', id], '/data', '1', h.options);
  await vi.waitFor(() => expect(h.out.text()).toContain('[y/N]'));
  h.input.write(answer);
  expect(await pending).toBe(1); expect(h.request).toHaveBeenCalledTimes(1);
});
it.each(['', 'yes'])('cancels on EOF including an unterminated %j answer', async answer => {
  const h = harness();
  const pending = runControlCli(['settle', id], '/data', '1', h.options);
  await vi.waitFor(() => expect(h.out.text()).toContain('[y/N]'));
  h.input.end(answer);
  expect(await pending).toBe(1); expect(h.request).toHaveBeenCalledTimes(1);
});
it('refuses non-TTY confirmations without consuming piped yes', async () => {
  const h = harness(false); h.input.end('yes\n');
  expect(await runControlCli(['delete', id], '/data', '1', h.options)).toBe(1);
  expect(h.request).toHaveBeenCalledTimes(1); expect(h.out.text()).not.toContain('[y/N]');
});
it('also refuses redirected output and ended TTY input', async () => {
  const h = harness(); h.out.stream.isTTY = false;
  expect(await confirmControl('question', h.input, h.out.stream)).toBe(false);
  h.out.stream.isTTY = true; h.input.destroy();
  expect(await confirmControl('question', h.input, h.out.stream)).toBe(false);
});
it('cancels on input error and refuses closed output', async () => {
  const h = harness();
  const pending = confirmControl('question', h.input, h.out.stream);
  h.input.emit('error', new Error('input lost'));
  expect(await pending).toBe(false);
  h.out.stream.destroy();
  expect(await confirmControl('question', h.input, h.out.stream)).toBe(false);
});
it('reports a failed confirmed action instead of claiming success', async () => {
  const h = harness();
  h.request.mockResolvedValueOnce({ session: session(), tabs: response.tabs }).mockRejectedValueOnce(new Error('closure failed'));
  const pending = runControlCli(['delete', id], '/data', '1', h.options);
  await vi.waitFor(() => expect(h.out.text()).toContain('[y/N]'));
  h.input.write('y\n');
  expect(await pending).toBe(1); expect(h.request).toHaveBeenLastCalledWith('/data', { action: 'delete', id, confirmed: true });
  expect(h.err.text()).toContain('closure failed'); expect(h.out.text()).not.toContain('Session deleted');
});
it('stops on failed inspection and escapes control bytes in server errors', async () => {
  const h = harness(); h.request.mockRejectedValue(new Error('broken\x1b[2J\rserver\x9b'));
  expect(await runControlCli(['delete', id], '/data', '1', h.options)).toBe(1);
  expect(h.request).toHaveBeenCalledTimes(1); expect(h.err.text()).toContain('\\u001b');
  expect(h.err.text()).not.toMatch(/[\x1b\r\x9b]/); expect(h.out.text()).toBe('');
});
it('does not mutate an uninspectable or mismatched target', async () => {
  for (const inspected of [{}, { session: session({ id: otherId }) }]) {
    const h = harness(); h.request.mockResolvedValue(inspected);
    expect(await runControlCli(['delete', id], '/data', '1', h.options)).toBe(1);
    expect(h.request).toHaveBeenCalledTimes(1);
  }
});
