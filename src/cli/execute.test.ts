import { randomUUID } from 'node:crypto';
import { EventEmitter } from 'node:events';
import { PassThrough, Writable } from 'node:stream';
import path from 'node:path';
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import { afterEach, expect, it, vi } from 'vitest';
import { EmbeddedSessionService } from '../main/terminal/service';
import { PtyBackend } from '../main/terminal/backend';
import { factoryFixture, MemoryRepository } from '../main/terminal/test-fixtures';
import { startHostRpc } from '../host/rpc';
import { connectHostClient } from '../host/client';
import { executeCommand } from './execute';
import { parseCommand } from './commands';
import { attachTerminal } from './attach';
vi.mock('./attach', () => ({ attachTerminal: vi.fn(async () => {}) }));
class Input extends PassThrough {
  isTTY = true; isRaw = false;
  setRawMode(value: boolean) { this.isRaw = value; return this; }
}
class Output extends Writable {
  isTTY = true; columns = 80; rows = 24;
  override _write(_chunk: Buffer, _encoding: BufferEncoding, done: () => void) { done(); }
}
const cleanup: Array<() => Promise<unknown> | void> = [];
afterEach(async () => { vi.restoreAllMocks(); vi.clearAllMocks(); for (const action of cleanup.splice(0).reverse()) await action(); });
async function fixture() {
  await mkdir('tmp', { recursive: true });
  const userData = await mkdtemp(path.resolve('tmp/cli-attach-'));
  cleanup.push(() => rm(userData, { recursive: true, force: true }));
  const pty = factoryFixture(), repository = new MemoryRepository();
  const service = new EmbeddedSessionService(repository, new PtyBackend(pty.options), () => ({ setWatch() {}, dispose() {} }));
  await service.initialize(); cleanup.push(() => service.dispose());
  const rpc = await startHostRpc({ userData, service, shutdown: async () => {}, mode: 'headless' });
  cleanup.push(() => rpc.close());
  const host = await connectHostClient(userData); cleanup.push(() => host.close());
  const io = { input: new Input(), output: new Output(), signals: new EventEmitter() };
  const create = async () => {
    const result = await service.createSession({ cwd: process.cwd(), requestId: randomUUID() });
    if (!result.ok) throw new Error(result.error.message);
    return result.value.tabs[0];
  };
  const run = async (args: string[], json = false) => {
    const command = parseCommand(['terminals', 'attach', ...args]);
    if (!command || command.kind !== 'terminal' || command.operation !== 'attach') throw new Error('Expected attach command');
    return executeCommand(command, host, { json, yes: false, userData }, io);
  };
  const write = async (tabId: string, text: string, take = false) => {
    const command = parseCommand(['terminals', 'write', tabId, text, ...(take ? ['--take'] : [])]);
    if (!command) throw new Error('Expected terminal write');
    return executeCommand(command, host, { json: false, yes: false, userData }, io);
  };
  return { service, repository, pty, io, run, write, create, host, userData };
}
it('attaches the explicit tab without creating sessions/tabs or resolving a directory', async () => {
  const f = await fixture(), tab = await f.create();
  const before = f.repository.sessions(), tabs = f.repository.tabs(tab.sessionId);
  const create = vi.spyOn(f.service, 'createSession'), add = vi.spyOn(f.service, 'addTab'), directory = vi.spyOn(f.host.service, 'resolveDirectory');
  await f.run([tab.id]);
  expect(attachTerminal).toHaveBeenCalledExactlyOnceWith(expect.anything(), tab.id, f.io);
  expect(create).not.toHaveBeenCalled(); expect(add).not.toHaveBeenCalled(); expect(directory).not.toHaveBeenCalled();
  expect(f.repository.sessions()).toEqual(before); expect(f.repository.tabs(tab.sessionId)).toEqual(tabs);
  expect(f.pty.processes[0].kill).not.toHaveBeenCalled();
});
it.each([[], ['--cwd', '.'], ['--session-id', '11111111-1111-4111-8111-111111111111']])('rejects missing IDs and creation options without mutations: %j', async (...args) => {
  const f = await fixture();
  const create = vi.spyOn(f.service, 'createSession'), add = vi.spyOn(f.service, 'addTab');
  await expect(f.run(args)).rejects.toMatchObject({ code: 'VALIDATION' });
  expect(create).not.toHaveBeenCalled(); expect(add).not.toHaveBeenCalled();
  expect(f.repository.sessions()).toHaveLength(0); expect(f.pty.factory).not.toHaveBeenCalled(); expect(attachTerminal).not.toHaveBeenCalled();
});
it.each(['json', 'input', 'output'] as const)('rejects %s before attaching or mutating the existing tab', async reason => {
  const f = await fixture(), tab = await f.create();
  if (reason === 'input') f.io.input.isTTY = false;
  if (reason === 'output') f.io.output.isTTY = false;
  await expect(f.run([tab.id], reason === 'json')).rejects.toThrow(reason === 'json' ? '--json' : 'TTY');
  expect(f.repository.sessions()).toHaveLength(1); expect(f.repository.tabs(tab.sessionId)).toHaveLength(1); expect(attachTerminal).not.toHaveBeenCalled();
});
it('writes once to an unoccupied terminal and releases its temporary view', async () => {
  const f = await fixture(), tab = await f.create();
  f.pty.processes[0].write.mockClear();
  await f.write(tab.id, 'one-shot input');
  expect(f.pty.processes[0].write).toHaveBeenCalledExactlyOnceWith('one-shot input');
  const next = await connectHostClient(f.userData); cleanup.push(() => next.close());
  expect(await next.service.attachTerminal({ tabId: tab.id })).toMatchObject({ ok: true, value: { control: 'owned' } });
});
it('one-shot write refuses another owner unless --take explicitly switches control', async () => {
  const f = await fixture(), tab = await f.create();
  const viewer = await connectHostClient(f.userData); cleanup.push(() => viewer.close());
  expect(await viewer.service.attachTerminal({ tabId: tab.id })).toMatchObject({ ok: true, value: { control: 'owned' } });
  f.pty.processes[0].write.mockClear();
  await expect(f.write(tab.id, 'not sent')).rejects.toMatchObject({ code: 'TERMINAL_CONTROL_REQUIRED' });
  expect(f.pty.processes[0].write).not.toHaveBeenCalled();
  await f.write(tab.id, 'explicit takeover', true);
  expect(f.pty.processes[0].write).toHaveBeenCalledExactlyOnceWith('explicit takeover');
  expect(await viewer.service.attachTerminal({ tabId: tab.id })).toMatchObject({ ok: true, value: { control: 'owned' } });
});
