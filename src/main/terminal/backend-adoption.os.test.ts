import { randomUUID } from 'node:crypto';
import { spawn } from 'node:child_process';
import { StringDecoder } from 'node:string_decoder';
import { expect, it } from 'vitest';
import type { Result } from '../../shared/contracts';
import { PtyBackend, type PtyProcess, type TerminalAdoption } from './backend';

const value = <T>(result: Result<T>): T => { if (!result.ok) throw new Error(result.error.code); return result.value; };
const identity = (): TerminalAdoption => ({ tabId: randomUUID(), sessionId: randomUUID(), generation: randomUUID(), cwd: '/work' });
// OWNED INTEGRATION HOLD: this file is never selected by default unit.
it('renders a real pipe child without altering the captured stdout and stderr bytes', async () => {
  const stdout = Buffer.from([0, 65, 10, 0xf0, 0x9f, 0x99, 0x82]), stderr = Buffer.from([66, 13, 10, 0xff]);
  const child = spawn(process.execPath, ['-e', `process.stdout.write(Buffer.from(${JSON.stringify([...stdout])})); process.stderr.write(Buffer.from(${JSON.stringify([...stderr])}));`], { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
  if (!child.pid) throw new Error('Child did not start');
  const captured = { stdout: [] as Buffer[], stderr: [] as Buffer[] };
  const listeners = new Set<(data: string) => void>(), exits = new Set<(event: { exitCode: number }) => void>();
  for (const stream of ['stdout', 'stderr'] as const) {
    const decoder = new StringDecoder('utf8');
    child[stream].on('data', (data: Buffer) => { captured[stream].push(data); for (const listener of listeners) listener(decoder.write(data)); });
    child[stream].on('end', () => { const tail = decoder.end(); if (tail) for (const listener of listeners) listener(tail); });
  }
  const closed = new Promise<void>((resolve, reject) => {
    child.once('error', reject);
    child.once('close', code => { for (const listener of [...exits]) listener({ exitCode: code ?? 1 }); resolve(); });
  });
  const adapter: PtyProcess = { pid: child.pid, write: () => { throw new Error('read-only'); }, resize: () => {}, kill: () => { child.kill(); },
    onData: listener => { listeners.add(listener); return { dispose: () => { listeners.delete(listener); } }; },
    onExit: listener => { exits.add(listener); return { dispose: () => { exits.delete(listener); } }; },
  };
  const backend = new PtyBackend(), input = identity();
  try {
    value(backend.adopt(input, adapter));
    await closed;
    expect(Buffer.concat(captured.stdout)).toEqual(stdout); expect(Buffer.concat(captured.stderr)).toEqual(stderr);
    const replay = value(backend.attach({ tabId: input.tabId }));
    const text = replay.chunks.map(chunk => chunk.data).join('');
    expect(text).toContain(stdout.toString('utf8')); expect(text).toContain(stderr.toString('utf8'));
    expect(replay).toMatchObject({ state: 'closed', exitCode: 0 });
  } finally { child.kill(); await closed; await backend.dispose(); }
});
