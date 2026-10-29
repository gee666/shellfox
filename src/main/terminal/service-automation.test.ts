import { randomUUID } from 'node:crypto';
import { expect, it, vi } from 'vitest';
import type { ChangedEvent, Result } from '../../shared/contracts';
import { sessionSchema, snapshotSchema } from '../../shared/schemas';
import { PtyBackend } from './backend';
import { EmbeddedSessionService } from './service';
import { FakePty, MemoryRepository, factoryFixture } from './test-fixtures';

const value = <T>(result: Result<T>): T => { if (!result.ok) throw new Error(result.error.code); return result.value; };
async function fixture(repository = new MemoryRepository()) {
  const f = factoryFixture(), backend = new PtyBackend(f.options), tracker = { setWatch: vi.fn(), dispose: vi.fn() };
  const service = new EmbeddedSessionService(repository, backend, () => tracker);
  await service.initialize();
  const changes: ChangedEvent[] = []; service.subscribe(event => changes.push(event));
  return { ...f, backend, service, repository, tracker, changes };
}

it('publishes a durable automation session through the ordinary snapshot without spawning a shell', async () => {
  const f = await fixture(), pty = new FakePty(), requestId = randomUUID();
  // Adoption does not depend on an installed interactive profile.
  f.service.probe.available = false;
  try {
    const session = value(await f.service.createAutomationTerminal({ cwd: '/work', title: 'Build', requestId }, pty)), tab = session.tabs[0];
    expect(sessionSchema.safeParse(session).success).toBe(true);
    expect(snapshotSchema.safeParse(value(f.service.getSnapshot())).success).toBe(true);
    expect(session).toMatchObject({ title: 'Build', canFocus: true, canAddTab: false, adapterId: 'embedded-pty' });
    expect(tab).toMatchObject({ title: 'Build', cwd: '/work', profileId: 'automation-pipe', lifecycle: 'open', terminalKind: 'embedded' });
    expect(f.repository.sessionByRequest(requestId)?.id).toBe(session.id);
    expect(f.repository.tab(tab.id)?.terminal?.userTitle).toBe('Build');
    expect(f.changes.at(-1)?.selectSessionId).toBe(session.id);
    expect(f.tracker.setWatch).toHaveBeenLastCalledWith([expect.objectContaining({ pid: pty.pid })], expect.any(Array));
    pty.output('build output\n');
    const marker = f.backend.get(tab.id)!.root.marker;
    expect(await f.service.controlSession({ action: 'tab-open', marker })).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
    const write = { tabId: tab.id, generation: tab.generation!, data: 'unwanted stdin' };
    expect(f.service.writeTerminal(write)).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
    const view = f.service.createTerminalView(() => {}), attachment = value(view.attach({ tabId: tab.id }));
    expect(attachment.control).toBe('owned');
    expect(view.write({ ...write, controlEpoch: attachment.controlEpoch })).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
    expect(pty.write).not.toHaveBeenCalled();
    pty.exit(7);
    expect(value(f.service.getSnapshot()).sessions[0].tabs[0]).toMatchObject({ lifecycle: 'closed', exitCode: 7 });
    expect(value(f.service.attachTerminal({ tabId: tab.id })).chunks[0].data).toBe('build output\n');
    expect(value(await f.service.focusSession({ sessionId: session.id }))).toEqual({ focused: true });
    expect(f.changes.at(-1)?.selectSessionId).toBe(session.id);
    expect(value(await f.service.activateSession({ sessionId: session.id })).tabs[0].lifecycle).toBe('closed');
    expect(await f.service.retryTab({ tabId: tab.id, confirmPossibleDuplicate: true })).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
    expect(await f.service.addTab({ sessionId: session.id, profileId: 'login-shell' })).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
    expect(f.factory).not.toHaveBeenCalled();
    expect(value(await f.service.closeTab({ tabId: tab.id, generation: tab.generation! })).tabs).toEqual([]);
    expect(pty.kill).not.toHaveBeenCalled();
    view.dispose();
  } finally { await f.service.dispose(); }
});

it('adopts an already-exited run with deferred replay and deduplicates concurrent show requests', async () => {
  const f = await fixture(), pty = new FakePty(), requestId = randomUUID();
  const onData = vi.spyOn(pty, 'onData').mockImplementation(listener => {
    queueMicrotask(() => listener('buffered stdout\nbuffered stderr\n'));
    return { dispose: vi.fn() };
  });
  const onExit = vi.spyOn(pty, 'onExit').mockImplementation(listener => {
    queueMicrotask(() => listener({ exitCode: 12 }));
    return { dispose: vi.fn() };
  });
  try {
    const input = { cwd: '/work', requestId };
    const [first, duplicate] = await Promise.all([f.service.createAutomationTerminal(input, pty), f.service.createAutomationTerminal(input, pty)]);
    const session = value(first), tab = session.tabs[0];
    expect(value(duplicate).id).toBe(session.id);
    expect(tab).toMatchObject({ title: 'Automation', lifecycle: 'closed', exitCode: 12 });
    expect(onData).toHaveBeenCalledTimes(1); expect(onExit).toHaveBeenCalledTimes(1);
    expect(value(f.service.attachTerminal({ tabId: tab.id }))).toMatchObject({ state: 'closed', chunks: [expect.objectContaining({ data: 'buffered stdout\nbuffered stderr\n' })] });
    expect(value(await f.service.focusSession({ sessionId: session.id }))).toEqual({ focused: true });
    expect(f.factory).not.toHaveBeenCalled();
  } finally { await f.service.dispose(); }
});

it('never reopens an automation session as a shell after the host restarts', async () => {
  const f = await fixture(), pty = new FakePty();
  const session = value(await f.service.createAutomationTerminal({ cwd: '/work', requestId: randomUUID() }, pty));
  await f.service.dispose();
  const restarted = await fixture(f.repository);
  try {
    expect(restarted.repository.tabs(session.id)).toEqual([]);
    expect(value(await restarted.service.activateSession({ sessionId: session.id })).tabs).toEqual([]);
    expect(await restarted.service.addTab({ sessionId: session.id })).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
    expect(restarted.factory).not.toHaveBeenCalled();
  } finally { await restarted.service.dispose(); }
});

it('validates automation requests and does not touch a caller-owned process on rejection', async () => {
  const f = await fixture(), pty = new FakePty();
  try {
    expect(await f.service.createAutomationTerminal({ cwd: 'relative', title: 'Build', requestId: randomUUID() }, pty)).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
    expect(await f.service.createAutomationTerminal({ cwd: '/work', title: '', requestId: randomUUID() }, pty)).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
    expect(f.repository.sessions()).toEqual([]); expect(pty.kill).not.toHaveBeenCalled();
    const requestId = randomUUID();
    value(await f.service.createSession({ cwd: '/work', requestId }));
    expect(await f.service.createAutomationTerminal({ cwd: '/work', requestId }, pty)).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
    expect(pty.kill).not.toHaveBeenCalled();
  } finally { await f.service.dispose(); }
});
