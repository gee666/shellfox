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
it.each(['session', 'ss', 'sessions'])('parses the %s namespace without top-level operation aliases', namespace => {
  for (const operation of ['list', 'stats', 'processes', 'delete', 'settle', 'unsettle']) expect(parseControlArgs([namespace, operation])).toMatchObject({ kind: 'control', action: operation });
  for (const operation of ['rm', '-rm']) expect(parseControlArgs([namespace, operation])).toMatchObject({ action: 'delete' });
});
it.each(['--version', '-v', 'version'])('parses version %s', alias => expect(parseControlArgs([alias])).toEqual({ kind: 'version' }));
it('supports root and canonical group help without contacting the app', () => {
  for (const flag of ['--help', '-h', 'help']) {
    expect(parseControlArgs([flag])).toEqual({ kind: 'help' });
    expect(parseControlArgs(['session', 'stats', flag])).toEqual({ kind: 'help', topic: 'session stats' });
  }
  expect(parseControlArgs(['help', 'terminals'])).toEqual({ kind: 'help', topic: 'terminals' });
});
it('uses optional local context, explicit IDs always win, and two explicit IDs are rejected', () => {
  for (const operation of ['delete', 'settle', 'unsettle', 'stats', 'processes']) {
    expect(parseControlArgs(['session', operation])).toMatchObject({ filters: { current: true } });
    expect(parseControlArgs(['ss', operation, id, '--current'])).toEqual({ kind: 'control', action: operation, filters: { id } });
    expect(parseControlArgs(['session', operation, '--id=' + id.toUpperCase()])).toMatchObject({ filters: { id } });
    expect(() => parseControlArgs(['session', operation, id, '--id', otherId])).toThrow();
  }
  expect(parseControlArgs(['session', 'list'])).toMatchObject({ filters: {} });
  expect(parseControlArgs(['session', 'stats', '--all'])).toMatchObject({ filters: { all: true } });
});
it('combines filters and accepts equals syntax without ambient-marker intersection', () => {
  expect(parseControlArgs(['session', 'processes', '--running', '--no-agents', '--current', '--id', id, '--name=proj', '--path', '/work', '--tab', tabId, '--pid=42'])).toEqual({
    kind: 'control', action: 'processes', filters: { running: true, noAgents: true, id, name: 'proj', path: '/work', tab: tabId, pid: 42 },
  });
});
it.each(['start','add','new','create','ls','list','stats','status','processes','proc','ps','delete','remove','rm','settle','unsettle','tab','keep-alive','awake','constructor','__proto__'])('rejects removed top-level %s', command => expect(() => parseControlArgs([command])).toThrow());
it.each([
  ['wat'], ['archive', '--help'], ['help', 'archive'], ['session', 'status'],
  ['session', 'list', '--wat'], ['session', 'list', '--running', '--idle'], ['session', 'stats', '--pid', '42'],
  ['session', 'list', '--id', '1'], ['session', 'list', '--tab', 'abc'], ['session', 'processes', '--pid', '0'],
  ['session', 'processes', '--pid', '1.1'], ['session', 'processes', '--pid', '9007199254740992'],
  ['session', 'list', '--name'], ['session', 'list', '--name', ''], ['session', 'list', '--name', 'a', '--name', 'b'],
  ['session', 'list', '--running=true'], ['session', 'list', '--running', '--running'],
  ['session', 'delete', '--yes'], ['session', 'delete', '--running'], ['session', 'settle', '--yes'],
  ['session', 'unsettle', '--running'], ['session', 'unsettle', 'Project'], ['session', 'delete', id, '--id', id],
  ['session', 'delete', 'Project'], ['session', 'stats', '--all', '--current'], ['session', 'stats', id, '--all'],
  ['version', 'extra'], ['help', 'missing'], ['session', 'stats', '--name', 'prod', '--help'],
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
it('explicit IDs override current context instead of intersecting it', async () => {
  const h = harness();
  expect(await runControlCli(['session', 'list', '--current', '--id', otherId], '/data', '1.2.3', h.options)).toBe(0);
  expect(h.request).toHaveBeenCalledWith('/data', { action: 'list', id: otherId });
  h.request.mockResolvedValue({ sessions: [session()] });
  const out = output();
  expect(await runControlCli(['session', 'list', '--current', '--id', otherId], '/data', '1', { ...h.options, output: out.stream })).toBe(0);
  expect(out.text()).toContain('No matching sessions');
});
it.each(['tab', 'awake', 'keep-alive'])('rejects removed top-level %s without contacting the host', async command => {
  const h = harness(); expect(await runControlCli([command], '/data', '1', h.options)).toBe(2); expect(h.request).not.toHaveBeenCalled();
});
it.each(['settle', 'unsettle'])('dispatches %s directly without inspection, TTY or confirmation', async action => {
  for (const args of [['session', action], ['session', action, '--current'], ['session', action, id], ['session', action, '--id', id]]) {
    const h = harness(false, { message: 'Status changed.' }); h.input.end('no\n');
    expect(await runControlCli(args, '/data', '1', h.options)).toBe(0);
    expect(h.request).toHaveBeenCalledExactlyOnceWith('/data', args.includes(id)
      ? { action, id } : { action, current: true, marker: h.options.marker });
    expect(h.out.text()).toContain('Status changed.'); expect(h.out.text()).not.toContain('[y/N]');
  }
});
it('help/version never contact the running app', async () => {
  const h = harness();
  for (const args of [[], ['-h'], ['help', 'terminals'], ['-v'], ['session', '-h']]) {
    expect(await runControlCli(args, '/data', '1.2.3', h.options)).toBe(0);
  }
  expect(h.request).not.toHaveBeenCalled(); expect(h.out.text()).toContain('1.2.3');
});
it('requires a marker for current operations and makes no requests on invalid syntax', async () => {
  const h = harness();
  for (const args of [['session', 'delete'], ['session', 'settle'], ['session', 'unsettle'], ['session', 'list', '--current'], ['session', 'stats'], ['session', 'list', '--pid', '42']]) {
    expect(await runControlCli(args, '/data', '1', { ...h.options, marker: undefined })).toBe(2);
  }
  expect(h.request).not.toHaveBeenCalled();
});
it('inspects delete, prompts, then pins the confirmed UUID', async () => {
  const h = harness();
  const pending = runControlCli(['session', 'delete'], '/data', '1', h.options);
  await vi.waitFor(() => expect(h.out.text()).toContain('[y/N]'));
  expect(h.request).toHaveBeenCalledTimes(1);
  expect(h.request).toHaveBeenLastCalledWith('/data', { action: 'inspect', marker: h.options.marker, current: true });
  expect(h.out.text()).toContain('Project'); expect(h.out.text()).toContain('PID 42');
  h.input.write('YES\n');
  expect(await pending).toBe(0);
  expect(h.request).toHaveBeenLastCalledWith('/data', { action: 'delete', id, confirmed: true });
});
it.each(['n\n', '\n', 'yep\n', 'true\n'])('cancels destructive operations on answer %j', async answer => {
  const h = harness();
  const pending = runControlCli(['session', 'rm', id], '/data', '1', h.options);
  await vi.waitFor(() => expect(h.out.text()).toContain('[y/N]'));
  h.input.write(answer);
  expect(await pending).toBe(1); expect(h.request).toHaveBeenCalledTimes(1);
});
it.each(['', 'yes'])('cancels on EOF including an unterminated %j answer', async answer => {
  const h = harness();
  const pending = runControlCli(['session', 'delete', id], '/data', '1', h.options);
  await vi.waitFor(() => expect(h.out.text()).toContain('[y/N]'));
  h.input.end(answer);
  expect(await pending).toBe(1); expect(h.request).toHaveBeenCalledTimes(1);
});
it('refuses non-TTY confirmations without consuming piped yes', async () => {
  const h = harness(false); h.input.end('yes\n');
  expect(await runControlCli(['session', 'delete', id], '/data', '1', h.options)).toBe(2);
  expect(h.request).not.toHaveBeenCalled(); expect(h.out.text()).not.toContain('[y/N]');
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
  const pending = runControlCli(['session', 'delete', id], '/data', '1', h.options);
  await vi.waitFor(() => expect(h.out.text()).toContain('[y/N]'));
  h.input.write('y\n');
  expect(await pending).toBe(1); expect(h.request).toHaveBeenLastCalledWith('/data', { action: 'delete', id, confirmed: true });
  expect(h.err.text()).toContain('closure failed'); expect(h.out.text()).not.toContain('Session deleted');
});
it('stops on failed inspection and escapes control bytes in server errors', async () => {
  const h = harness(); h.request.mockRejectedValue(new Error('broken\x1b[2J\rserver\x9b'));
  expect(await runControlCli(['session', 'delete', id], '/data', '1', h.options)).toBe(1);
  expect(h.request).toHaveBeenCalledTimes(1); expect(h.err.text()).toContain('\\u001b');
  expect(h.err.text()).not.toMatch(/[\x1b\r\x9b]/); expect(h.out.text()).toBe('');
});
it('does not mutate an uninspectable or mismatched target', async () => {
  for (const inspected of [{}, { session: session({ id: otherId }) }]) {
    const h = harness(); h.request.mockResolvedValue(inspected);
    expect(await runControlCli(['session', 'delete', id], '/data', '1', h.options)).toBe(1);
    expect(h.request).toHaveBeenCalledTimes(1);
  }
});
