import { EventEmitter } from 'node:events';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { expect, it, vi } from 'vitest';
const mock = vi.hoisted(() => ({ child: undefined as unknown as EventEmitter & { kill: ReturnType<typeof vi.fn>; unref: ReturnType<typeof vi.fn> } }));
vi.mock('node:child_process', () => ({ spawn: () => { queueMicrotask(() => mock.child.emit('spawn')); return mock.child; } }));
import { startHandoff } from './handoff';
it('does not mistake failed kill error events for exit or leave repeated kill errors unhandled', async () => {
  await mkdir('tmp', { recursive: true }); const control = await mkdtemp(path.resolve('tmp/update-kill-failure-'));
  try {
    mock.child = Object.assign(new EventEmitter(), { kill: vi.fn(() => { mock.child.emit('error', new Error('EACCES')); return false; }), unref: vi.fn() });
    await writeFile(path.join(control, 'ready'), '');
    const lease = await startHandoff('mock-helper', [], control, 100);
    await expect(lease.cancel()).rejects.toThrow('cancellation was not confirmed');
    expect(mock.child.kill.mock.calls).toEqual([[], ['SIGKILL']]);
    mock.child.emit('exit', 1); await expect(lease.cancel()).resolves.toBeUndefined();
    expect(lease.committed).toBe(false);
  } finally { await rm(control, { recursive: true, force: true }); }
});
it('never accepts a commit that races cancellation', async () => {
  await mkdir('tmp', { recursive: true }); const control = await mkdtemp(path.resolve('tmp/update-commit-cancel-'));
  try {
    mock.child = Object.assign(new EventEmitter(), { kill: vi.fn(() => { mock.child.emit('exit', 1); return true; }), unref: vi.fn() });
    await writeFile(path.join(control, 'ready'), ''); await writeFile(path.join(control, 'committed'), '');
    const lease = await startHandoff('mock-helper', [], control, 100);
    const committing = expect(lease.commit()).rejects.toThrow();
    await lease.cancel(); await committing;
    expect(lease.committed).toBe(false); expect(mock.child.unref).not.toHaveBeenCalled();
  } finally { await rm(control, { recursive: true, force: true }); }
});
