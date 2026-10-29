import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { parseGlobals } from './arguments';
import { parseAutomationCommand, runAutomationCommand } from './automation';
import { EmbeddedSessionService } from '../main/terminal/service';
import { PtyBackend } from '../main/terminal/backend';
import { factoryFixture, MemoryRepository } from '../main/terminal/test-fixtures';
import { startHostRpc } from '../host/rpc';
import { connectHostClient } from '../host/client';

const cleanups: Array<() => Promise<unknown>> = [];
afterEach(async () => { for (const action of cleanups.splice(0).reverse()) await action(); });
async function fixture() {
  await mkdir('tmp', { recursive: true }); const userData = await mkdtemp(path.resolve('tmp/automation-cli-'));
  cleanups.push(() => rm(userData, { recursive: true, force: true }));
  const pty = factoryFixture(), service = new EmbeddedSessionService(new MemoryRepository(), new PtyBackend(pty.options), () => ({ setWatch() {}, dispose() {} }));
  await service.initialize(); cleanups.push(() => service.dispose());
  const rpc = await startHostRpc({ userData, service, mode: 'desktop', shutdown: async () => {} }); cleanups.push(() => rpc.close());
  const client = await connectHostClient(userData); cleanups.push(() => client.close());
  const input = new PassThrough(), output = new PassThrough(), error = new PassThrough(), signals = new EventEmitter();
  const stdout: Buffer[] = [], stderr: Buffer[] = [];
  output.on('data', chunk => stdout.push(Buffer.from(chunk))); error.on('data', chunk => stderr.push(Buffer.from(chunk)));
  return { client, service, pty, io: { input, output, error, signals }, stdout, stderr };
}
describe('automation command', () => {
  it('protects literal program flags and values from global parsing', () => {
    const parsed = parseGlobals(['--json', 'run', '--title', '--yes', '--detach', '--', 'node', '--json', '--user-data', 'literal']);
    expect(parsed.options).toMatchObject({ json: true, yes: false });
    expect(parseAutomationCommand(parsed.args)).toEqual({ kind: 'run', executable: 'node', args: ['--json', '--user-data', 'literal'], cwd: undefined, title: '--yes', detached: true });
  });
  it('accepts a caller-known run ID without treating program arguments as options', () => {
    const id = '11111111-1111-4111-8111-111111111111';
    expect(parseAutomationCommand(['run', '--request-id', id, '--', 'node', '--request-id', 'literal'])).toMatchObject({ requestId: id, args: ['--request-id', 'literal'] });
    expect(() => parseAutomationCommand(['run', '--request-id', 'bad', '--', 'node'])).toThrow('UUID');
  });
  it('preserves raw stdin/stdout/stderr and child exit code through authenticated RPC', async () => {
    const f = await fixture();
    f.io.input.end(Buffer.from([0, 255, 10]));
    const code = await runAutomationCommand({ kind: 'run', executable: process.execPath, args: ['-e', "process.stdin.on('data',b=>process.stdout.write(b));process.stdin.on('end',()=>{process.stderr.write('error\\n');process.exitCode=9});"], detached: false }, f.client, { json: false, yes: false }, f.io);
    expect(code).toBe(9); expect(Buffer.concat(f.stdout)).toEqual(Buffer.from([0, 255, 10])); expect(Buffer.concat(f.stderr).toString()).toBe('error\n');
    expect(f.pty.processes).toHaveLength(0);
  });
  it('can add a read-only tab later without changing the running pipes', async () => {
    const f = await fixture(); f.io.input.end();
    const completed = runAutomationCommand({ kind: 'run', executable: process.execPath, args: ['-e', "process.stdout.write('before\\n');setTimeout(()=>{process.stderr.write('after\\n');process.exitCode=3},300);"], detached: false }, f.client, { json: false, yes: false }, f.io);
    let runs = await f.client.request('automation.list', {});
    for (let i = 0; runs.length === 0 && i < 100; i++) { await new Promise(resolve => setTimeout(resolve, 10)); runs = await f.client.request('automation.list', {}); }
    const view = await f.client.request('automation.show', { id: runs[0].id });
    expect(view.sessionId).not.toBeNull(); expect(view.tabId).not.toBeNull();
    expect(f.pty.processes).toHaveLength(0);
    expect(await completed).toBe(3); expect(Buffer.concat(f.stdout).toString()).toBe('before\n'); expect(Buffer.concat(f.stderr).toString()).toBe('after\n');
    const second = await f.client.request('automation.show', { id: view.id }); expect(second.sessionId).toBe(view.sessionId);
    const snapshot = f.service.getSnapshot(); if (!snapshot.ok) throw new Error('snapshot failed');
    const tab = snapshot.value.sessions.find(session => session.id === view.sessionId)?.tabs[0];
    if (!tab?.generation) throw new Error('Missing automation tab');
    const attachment = await f.client.service.attachTerminal({ tabId: tab.id, generation: tab.generation });
    expect(attachment.ok).toBe(true);
  });
  it('returns a detached run ID in JSON and does not open a shell', async () => {
    const f = await fixture();
    const code = await runAutomationCommand({ kind: 'run', executable: process.execPath, args: ['-e', 'setTimeout(()=>{},100)'], detached: true }, f.client, { json: true, yes: false }, f.io);
    expect(code).toBe(0); expect(JSON.parse(Buffer.concat(f.stdout).toString())).toMatchObject({ ok: true, value: { state: 'running', executable: process.execPath } });
    expect(f.pty.processes).toHaveLength(0);
  });
});
