import { randomUUID } from 'node:crypto';
import { describe, expect, it, vi } from 'vitest';
import type { Result } from '../../shared/contracts';
import { sessionSchema, titleSchema } from '../../shared/schemas';
import { PtyBackend, type TerminalTitleEvent } from './backend';
import { ScreenMirror } from './screen-mirror';
import { EmbeddedSessionService } from './service';
import { factoryFixture, MemoryRepository } from './test-fixtures';

const value = <T>(result: Result<T>): T => { if (!result.ok) throw new Error(result.error.code); return result.value; };
async function fixture() {
  const f = factoryFixture(), repository = new MemoryRepository(), backend = new PtyBackend(f.options);
  const service = new EmbeddedSessionService(repository, backend, () => ({ setWatch: () => {}, dispose: () => {} }));
  await service.initialize();
  const session = value(await service.createSession({ cwd: '/work', requestId: randomUUID() }));
  const current = () => value(service.getSnapshot()).sessions.find(item => item.id === session.id)!;
  return { ...f, repository, backend, service, session, current };
}

describe('terminal process titles', () => {
  it('parses OSC 0 and 2 with BEL and ST across writes, ignoring icon-only and incomplete sequences', () => {
    const changed = vi.fn(), mirror = new ScreenMirror(80, 24, changed);
    mirror.write(1, '\x1b]0;Build');
    expect(mirror.title).toBeNull();
    mirror.write(2, ' 雪\x07');
    expect(mirror.title).toBe('Build 雪');
    mirror.write(3, '\x1b]1;Icon only\x07');
    expect(mirror.title).toBe('Build 雪');
    mirror.write(4, '\x1b]2;Tests\x1b');
    // xterm completes OSC at ESC, before the trailing ST backslash arrives.
    expect(mirror.title).toBe('Tests');
    mirror.write(5, '\\');
    expect(mirror.title).toBe('Tests');
    mirror.write(6, '\x1b]2;Tests\x07');
    expect(changed.mock.calls).toEqual([['Build 雪'], ['Tests']]);
    mirror.write(7, '\x1b]2;Cancelled\x18');
    expect(mirror.title).toBe('Tests');
    mirror.dispose();
    mirror.write(8, '\x1b]2;After disposal\x07');
    expect(changed).toHaveBeenCalledTimes(2);
  });

  it('bounds process-controlled titles, removes controls, clears empty titles and isolates observers', () => {
    const mirror = new ScreenMirror(80, 24, () => { throw new Error('broken observer'); });
    mirror.write(1, '\x1b]2;  <build> 雪\x7f  \x07');
    expect(mirror.title).toBe('<build> 雪');
    expect(titleSchema.safeParse(mirror.title).success).toBe(true);
    mirror.write(2, '\x1b]2;' + 'x'.repeat(199) + '🦊' + '\x07');
    expect(mirror.title).toBe('x'.repeat(199));
    mirror.write(3, '\x1b]2;  \x07');
    expect(mirror.title).toBeNull();
    expect(mirror.sequence).toBe(3);
    mirror.dispose();
  });

  it('updates inactive tabs through session snapshots without attachment or database writes', async () => {
    const f = await fixture();
    const added = value(await f.service.addTab({ sessionId: f.session.id }));
    const [a, b] = added.tabs, changed = vi.fn();
    f.service.subscribe(changed);
    const saved = f.repository.tabs(), writes = vi.spyOn(f.repository, 'saveTab');
    f.processes[0].output('\x1b]2;Inactive build\x07');
    f.processes[1].output('\x1b]0;Active tests\x1b\\');
    expect(f.current().tabs.map(tab => tab.title)).toEqual(['Inactive build', 'Active tests']);
    expect(f.backend.get(a.id)?.processTitle).toBe('Inactive build');
    expect(f.backend.get(b.id)?.processTitle).toBe('Active tests');
    expect(sessionSchema.safeParse(f.current()).success).toBe(true);
    expect(changed).toHaveBeenCalledTimes(2);
    f.processes[0].output('\x1b]2;Inactive build\x07');
    expect(changed).toHaveBeenCalledTimes(2);
    f.processes[0].output('\x1b]2; \x07');
    expect(f.current().tabs[0].title).toBe(a.title);
    expect(writes).not.toHaveBeenCalled();
    expect(f.repository.tabs()).toEqual(saved);
    await f.service.dispose();
  });

  it.each(['My build 雪', 'Shell 1'])('gives rename %s and explicit add-tab titles precedence over all later process titles', async name => {
    const f = await fixture(), tab = f.session.tabs[0];
    f.processes[0].output('\x1b]2;Process title\x07');
    const renaming = f.service.renameTab({ sessionId: f.session.id, tabId: tab.id, title: `  ${name}  ` });
    f.processes[0].output('\x1b]2;During rename\x07');
    expect(value(await renaming).tabs[0].title).toBe(name);
    const changed = vi.fn(); f.service.subscribe(changed);
    f.processes[0].output('\x1b]0;After rename\x07');
    f.processes[0].output('\x1b]2;\x07');
    expect(changed).not.toHaveBeenCalled();
    expect(f.current().tabs[0].title).toBe(name);
    expect(f.repository.tab(tab.id)?.terminal?.userTitle).toBe(name);
    value(await f.service.addTab({ sessionId: f.session.id, title: 'Explicit name' }));
    f.processes[1].output('\x1b]2;Must not replace explicit name\x07');
    expect(f.current().tabs[1].title).toBe('Explicit name');
    await f.service.dispose();
  });

  it('preserves overrides after restart while process titles remain tied to their runtime generation', async () => {
    const f = await fixture(), tab = f.session.tabs[0];
    value(await f.service.renameTab({ sessionId: f.session.id, tabId: tab.id, title: 'User name' }));
    const added = value(await f.service.addTab({ sessionId: f.session.id })), automatic = added.tabs[1];
    f.processes[1].output('\x1b]2;Previous command\x07');
    await f.service.dispose();
    const next = factoryFixture(), backend = new PtyBackend(next.options);
    const service = new EmbeddedSessionService(f.repository, backend, () => ({ setWatch: () => {}, dispose: () => {} }));
    await service.initialize();
    expect(service.toDto(f.repository.session(f.session.id)!).tabs.map(t => t.title)).toEqual(['User name', automatic.title]);
    // Reattach a test generation to the same saved record to verify persisted override precedence.
    value(await backend.launch({ tabId: tab.id, sessionId: f.session.id, generation: tab.generation!, profileId: 'login-shell', cwd: '/work' }));
    next.processes[0].output('\x1b]2;New command\x07');
    expect(service.toDto(f.repository.session(f.session.id)!).tabs[0].title).toBe('User name');
    await service.dispose();
  });

  it('preserves pre-upgrade saved names, including names indistinguishable from generated defaults', async () => {
    const f = await fixture(), tab = f.repository.tab(f.session.tabs[0].id)!;
    delete tab.terminal!.userTitle;
    f.repository.saveTab(tab);
    f.processes[0].output('\x1b]2;Cannot infer whether Shell 1 was a rename\x07');
    expect(f.current().tabs[0].title).toBe('Shell 1');
    f.repository.saveTab({ ...tab, title: 'Old user name' });
    f.processes[0].output('\x1b]2;Still protected\x07');
    expect(f.current().tabs[0].title).toBe('Old user name');
    await f.service.dispose();
  });

  it('does not establish an override when rename persistence fails', async () => {
    const f = await fixture(), tab = f.session.tabs[0];
    const write = vi.spyOn(f.repository, 'saveSession').mockImplementation(() => { throw new Error('disk full'); });
    expect(await f.service.renameTab({ sessionId: f.session.id, tabId: tab.id, title: 'Unsaved name' })).toMatchObject({ ok: false, error: { code: 'STORAGE_FAILED' } });
    write.mockRestore();
    f.processes[0].output('\x1b]2;Process still wins\x07');
    expect(f.current().tabs[0].title).toBe('Process still wins');
    expect(f.repository.tab(tab.id)?.terminal?.userTitle).toBeNull();
    await f.service.dispose();
  });

  it('rejects stale title notifications and output from retired PTYs', async () => {
    const f = factoryFixture(), repository = new MemoryRepository(), backend = new PtyBackend(f.options);
    let notify!: (event: TerminalTitleEvent) => void;
    const subscribe = backend.subscribeTitle.bind(backend);
    vi.spyOn(backend, 'subscribeTitle').mockImplementation(listener => { notify = listener; return subscribe(listener); });
    const service = new EmbeddedSessionService(repository, backend, () => ({ setWatch: () => {}, dispose: () => {} }));
    await service.initialize();
    const session = value(await service.createSession({ cwd: '/work', requestId: randomUUID() })), tab = session.tabs[0];
    const changed = vi.fn(); service.subscribe(changed);
    notify({ tabId: tab.id, generation: randomUUID(), title: 'Stale title' });
    expect(changed).not.toHaveBeenCalled();
    f.processes[0].exit(0);
    const generation = randomUUID();
    value(await backend.launch({ tabId: tab.id, sessionId: session.id, generation, profileId: 'login-shell', cwd: '/work' }));
    f.processes[0].output('\x1b]2;Retired PTY\x07');
    expect(backend.get(tab.id)?.processTitle).toBeNull();
    f.processes[1].output('\x1b]2;Uncommitted generation\x07');
    expect(service.toDto(repository.session(session.id)!).tabs[0].title).toBe(tab.title);
    repository.saveTab({ ...repository.tab(tab.id)!, operationId: generation });
    f.processes[1].output('\x1b]2;Current generation\x07');
    expect(service.toDto(repository.session(session.id)!).tabs[0].title).toBe('Current generation');
    await service.dispose();
  });
});
