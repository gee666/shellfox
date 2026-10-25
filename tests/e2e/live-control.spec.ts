import { test, expect, type ElectronApplication } from '@playwright/test';
import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { randomUUID } from 'node:crypto';
import { mkdir, writeFile, access } from 'node:fs/promises';
import path from 'node:path';
import { stripVTControlCharacters } from 'node:util';
import { spawn as spawnPty } from 'node-pty';
import { launch, scratch, snapshot, value, env, root, testMain } from '../fixtures/electron';
import { requestControl } from '../../src/main/control-client';
import type { OwnedTerminal } from '../../src/main/terminal/backend';

const helper = path.join(root, 'tmp/build-test/cli/main/cli.cjs');
const runtime = process.platform === 'win32' ? path.join(root, 'tmp/build-test/cli/cli-runtime/node.exe') : process.execPath;
const electron = createRequire(path.join(root, 'package.json'))('electron') as string;
const plain = (text: string) => stripVTControlCharacters(text).replace(/\r/g, '');
const quote = (text: string, powershell: boolean) => "'" + text.replaceAll("'", powershell ? "''" : "'\\''") + "'";
const live = (app: ElectronApplication): Promise<OwnedTerminal[]> => app.evaluate(() => (globalThis as any).__shellfoxTest.backend.live());
const identity = (terminals: OwnedTerminal[]) => terminals.map(t => [t.tabId, t.generation, t.root.pid]).sort((a, b) => String(a[0]).localeCompare(String(b[0])));
const replay = (app: ElectronApplication, tabId: string) => app.evaluate((_electron, id) => {
  const result = (globalThis as any).__shellfoxTest.backend.attach({ tabId: id });
  if (!result.ok) throw new Error(result.error.message);
  return result.value.chunks.map((chunk: { data: string }) => chunk.data).join('');
}, tabId).then(plain);

// All spawned helpers receive an explicit isolated data path. Never inherit the
// invoking developer terminal's marker or rely on default APPDATA/XDG paths.
function helperEnv(): Record<string, string> {
  const result: NodeJS.ProcessEnv = { ...env(), NO_COLOR: '1' };
  delete result.SHELLFOX_TERMINAL_MARKER;
  delete result.ELECTRON_RUN_AS_NODE;
  return Object.fromEntries(Object.entries(result).filter((entry): entry is [string, string] => typeof entry[1] === 'string'));
}
function pipe(command: string, args: string[], cwd: string, childEnv = helperEnv()): Promise<{ code: number | null; output: string }> {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd, env: childEnv, windowsHide: true, shell: false, stdio: 'pipe' });
    let output = '';
    const timer = setTimeout(() => { child.kill(); reject(new Error('Owned CLI exceeded 60s deadline: ' + plain(output))); }, 60000);
    child.stdout.on('data', data => { output += data; }); child.stderr.on('data', data => { output += data; });
    child.stdin.end();
    child.once('error', error => { clearTimeout(timer); reject(error); });
    child.once('close', code => { clearTimeout(timer); resolve({ code, output: plain(output) }); });
  });
}
function tty(data: string, args: string[], cwd: string, answer: 'y' | 'n'): Promise<{ code: number; output: string; answered: boolean }> {
  return new Promise((resolve, reject) => {
    const terminal = spawnPty(runtime, [helper, '--user-data', data, ...args], { cwd, env: helperEnv(), name: 'xterm-256color', cols: 180, rows: 50 });
    let output = '', answered = false;
    const timer = setTimeout(() => { terminal.kill(); reject(new Error('Owned TTY CLI exceeded 60s deadline: ' + plain(output))); }, 60000);
    terminal.onData(text => {
      output += text;
      if (!answered && plain(output).includes('? [y/N]')) { answered = true; terminal.write(answer + '\r'); }
    });
    terminal.onExit(({ exitCode }) => { clearTimeout(timer); setTimeout(() => resolve({ code: exitCode, output: plain(output), answered }), 100); });
  });
}

test('real Electron live CLI transport, inherited context, TTY confirmation and session closure', async ({}, testInfo) => {
  test.setTimeout(300000);
  const dir = await scratch('live-control'), data = path.join(dir, 'data');
  expect(path.relative(path.join(root, 'tmp'), dir)).not.toMatch(/^\.\./);
  const keepCwd = path.join(dir, 'control-keep'), settledCwd = path.join(dir, 'control-settled');
  await mkdir(keepCwd); await mkdir(settledCwd); await access(helper); await access(runtime);
  const running = await launch(data, 'real', false, ['start', keepCwd]);
  const { app, page } = running;
  const evidence: { userData: string; steps: unknown[] } = { userData: data, steps: [] };
  const record = (name: string, details: unknown) => evidence.steps.push({ name, details });
  const helperPipe = async (args: string[]) => {
    const result = await pipe(runtime, [helper, '--user-data', data, ...args], dir);
    record('helper ' + args.join(' '), result); return result;
  };
  async function waitForRoutedShell(cwd: string, sessions: number) {
    let last: unknown;
    try {
      await expect.poll(async () => {
        const state = await app.evaluate((_electron, input) => {
          const { repository, backend } = (globalThis as any).__shellfoxTest;
          const active = repository.sessions().filter((s: { settledAt: string | null }) => !s.settledAt);
          const session = active.find((s: { cwd: string }) => s.cwd === input.cwd);
          const tabs = session ? repository.tabs(session.id) : [];
          const owned = backend.live();
          const tab = tabs[0];
          return {
            sessions: active.length, roots: owned.length,
            ready: tabs.length === 1 && tab.lifecycle === 'open' && owned.some((t: OwnedTerminal) =>
              t.sessionId === session.id && t.tabId === tab.id && t.generation === tab.operationId && t.state === 'open'),
            tabs: tabs.map((t: { id: string; lifecycle: string; operationId: string; error: unknown }) =>
              ({ id: t.id, lifecycle: t.lifecycle, generation: t.operationId, error: t.error })),
          };
        }, { cwd });
        last = state;
        return { sessions: state.sessions, roots: state.roots, ready: state.ready };
      }, { timeout: 30000, message: 'App routing must finish PTY launch, not merely save its intent' }).toEqual({ sessions, roots: sessions, ready: true });
    } finally { record('routed shell readiness ' + cwd, last); }
  }
  const invoke = (terminal: OwnedTerminal, args: string[]) => {
    const powershell = ['pwsh', 'windows-powershell'].includes(terminal.profileId);
    expect(process.platform === 'win32' ? powershell : terminal.profileId === 'login-shell').toBe(true);
    return { powershell, command: (powershell ? '& ' : '') + [runtime, helper, '--user-data', data, ...args].map(token => quote(token, powershell)).join(' ') };
  };
  async function inShell(terminal: OwnedTerminal, args: string[]): Promise<string> {
    const { powershell, command } = invoke(terminal, args), token = randomUUID().replaceAll('-', '');
    const marker = 'CONTROL_RESULT_' + token;
    const suffix = powershell
      ? `; $controlCode=$LASTEXITCODE; [Console]::WriteLine(('CONTROL_RESULT_' + '${token}' + ':' + $controlCode))`
      : `; control_code=$?; printf '%s:%s\\n' '${marker}' "$control_code"`;
    value(await page.evaluate(input => window.shellfox.writeTerminal!(input), { tabId: terminal.tabId, generation: terminal.generation, data: command + suffix + '\r' }));
    await expect.poll(async () => (await replay(app, terminal.tabId)).includes(marker + ':0'), { timeout: 60000 }).toBe(true);
    const output = await replay(app, terminal.tabId);
    record('owned shell ' + args.join(' '), { tabId: terminal.tabId, generation: terminal.generation, marker: terminal.root.marker, output });
    return output;
  }
  async function confirmInside(terminal: OwnedTerminal, action: 'delete') {
    const command = invoke(terminal, [action, '--current']).command;
    value(await page.evaluate(input => window.shellfox.writeTerminal!(input), { tabId: terminal.tabId, generation: terminal.generation, data: command + '\r' }));
    await expect.poll(async () => (await replay(app, terminal.tabId)).includes(`${action} ${terminal.sessionId}? [y/N]`), { timeout: 60000 }).toBe(true);
    record('inherited-context TTY prompt ' + action, { terminal, output: await replay(app, terminal.tabId) });
    value(await page.evaluate(input => window.shellfox.writeTerminal!(input), { tabId: terminal.tabId, generation: terminal.generation, data: 'y\r' }));
  }
  try {
    expect(await app.evaluate(({ app }) => app.getPath('userData'))).toBe(data);
    const probe = (await snapshot(page)).probe;
    expect(probe.adapterId).toBe('embedded-pty'); expect(probe.available).toBe(true);
    await waitForRoutedShell(keepCwd, 1);
    const keep = (await snapshot(page)).sessions[0]!;
    expect(keep.cwd).toBe(keepCwd);
    // Warm alias goes through Electron single-instance/renderer routing. Gate
    // only its timing to reproduce intent-before-PTY deterministically; release
    // still runs the original backend/native factory, with no synthetic roots.
    await app.evaluate((_electron, cwd) => {
      const scope = globalThis as any, backend = scope.__shellfoxTest.backend;
      const original = backend.launch;
      let released = false, resume: (() => void) | undefined;
      const gate = scope.__liveControlLaunchGate = {
        entered: false,
        release() { released = true; resume?.(); backend.launch = original; },
      };
      backend.launch = async (input: { cwd: string }) => {
        if (input.cwd === cwd && !released) {
          gate.entered = true;
          await new Promise<void>(resolve => { resume = resolve; });
        }
        return original.call(backend, input);
      };
    }, settledCwd);
    try {
      const routed = await pipe(electron, [testMain, '--test-user-data', data, '--test-backend', 'real', 'add', settledCwd], dir);
      record('app routing add', routed); expect(routed.code).toBe(0);
      await expect.poll(() => app.evaluate(() => (globalThis as any).__liveControlLaunchGate.entered)).toBe(true);
      const intent = (await snapshot(page)).sessions.find(s => s.cwd === settledCwd)!;
      expect(intent.tabs[0]!.lifecycle).toBe('launching');
      expect((await snapshot(page)).sessions).toHaveLength(2);
      expect(await live(app)).toHaveLength(1);
      record('reproduced saved intent before second native launch', { session: intent, roots: await live(app) });
    } finally {
      await app.evaluate(() => {
        const scope = globalThis as any;
        scope.__liveControlLaunchGate?.release(); delete scope.__liveControlLaunchGate;
      });
    }
    // Waiting only for two DTOs raced here in the full suite. Native ownership
    // and persisted open/generation metadata must agree before CLI operations.
    await waitForRoutedShell(settledCwd, 2);
    const candidate = (await snapshot(page)).sessions.find(s => s.cwd === settledCwd)!;
    expect(candidate).toBeDefined();
    let roots = await live(app);
    expect(roots).toHaveLength(2);
    expect(roots.every(t => t.root.environment === 'local' && /^[0-9a-f-]{36}$/i.test(t.root.marker ?? '') && t.root.pid > 0)).toBe(true);
    record('native roots from real backend', roots);
    const originalKeep = roots.find(t => t.sessionId === keep.id)!;
    const keepIdentity = identity([originalKeep]);

    const listed = await helperPipe(['ls']);
    expect(listed.code).toBe(0); expect(listed.output).toContain(keep.id); expect(listed.output).toContain(candidate.id);
    const currentListing = await inShell(originalKeep, ['ls', '--current']);
    expect(currentListing).toContain(keep.id);
    await inShell(originalKeep, ['tab', 'open']);
    await expect.poll(async () => (await live(app)).filter(t => t.sessionId === keep.id).length).toBe(2);
    roots = await live(app);
    expect(identity(roots.filter(t => t.tabId === originalKeep.tabId))).toEqual(keepIdentity);
    const added = roots.find(t => t.sessionId === keep.id && t.tabId !== originalKeep.tabId)!;
    const addedCommand = invoke(added, ['tab', 'close']).command;
    value(await page.evaluate(input => window.shellfox.writeTerminal!(input), { tabId: added.tabId, generation: added.generation, data: addedCommand + '\r' }));
    await expect.poll(async () => (await live(app)).some(t => t.tabId === added.tabId), { timeout: 30000 }).toBe(false);
    expect(identity((await live(app)).filter(t => t.sessionId === keep.id))).toEqual(keepIdentity);
    record('current tab-close killed invoking CLI without closing siblings', { closed: added, remaining: await live(app) });

    for (const active of [true, false]) {
      const result = await helperPipe(['keep-alive']); expect(result.code).toBe(0);
      expect(value(await page.evaluate(() => window.shellfox.getKeepAwake!())).active).toBe(active);
      await expect(page.getByRole('button', { name: 'Keep awake' })).toHaveAttribute('aria-pressed', String(active));
      record('keep-alive updates the mounted eye button without reload', { active, ariaPressed: await page.getByRole('button', { name: 'Keep awake' }).getAttribute('aria-pressed') });
    }
    for (const action of ['stats', 'processes'] as const) {
      const result = await helperPipe([action]);
      expect(result.code).toBe(0); expect(result.output).toContain(keep.id); expect(result.output).toContain(candidate.id);
      expect(result.output).toContain('RSS'); expect(result.output).toContain('duration');
      const response = await requestControl(data, { action });
      expect(response.sessions?.map(s => s.id).sort()).toEqual([keep.id, candidate.id].sort());
      const owned = await live(app), tabIds = new Set(owned.map(t => t.tabId));
      expect(response.tabs).toHaveLength(owned.length);
      expect(response.tabs!.every(t => tabIds.has(t.tabId))).toBe(true);
      for (const tab of response.tabs!) {
        expect(tab.processes.length > 0 || !!tab.reason).toBe(true);
        if (!tab.processes.length) expect(tab.memoryBytes).toBeNull();
      }
      record('real transport ' + action, response);
    }
    const refused = await helperPipe(['delete', keep.id]);
    expect(refused.code).toBe(1); expect(refused.output).toContain('Cancelled. No changes made.');
    const cancelled = await tty(data, ['delete', keep.id], dir, 'n');
    record('external real PTY refuses confirmation', cancelled);
    expect(cancelled.answered).toBe(true); expect(cancelled.code).toBe(1); expect(cancelled.output).toContain('Cancelled.');
    expect(identity((await live(app)).filter(t => t.sessionId === keep.id))).toEqual(keepIdentity);

    value(await page.evaluate(input => window.shellfox.addTab(input), { sessionId: candidate.id }));
    const settledRoots = (await live(app)).filter(t => t.sessionId === candidate.id);
    expect(settledRoots).toHaveLength(2);
    const candidateIdentity = identity(settledRoots);
    // The invoking CLI must return normally, without confirmation or closing either shell.
    const insideSettle = await inShell(settledRoots[0]!, ['settle', '--current']);
    expect(insideSettle).not.toContain('? [y/N]');
    expect((await snapshot(page)).sessions.find(s => s.id === candidate.id)).toMatchObject({ status: 'settled', settledAt: expect.any(String) });
    expect(identity((await live(app)).filter(t => t.sessionId === candidate.id))).toEqual(candidateIdentity);
    expect((await requestControl(data, { action: 'list' })).sessions?.map(s => s.id).sort()).toEqual([keep.id, candidate.id].sort());
    const insideUnsettle = await inShell(settledRoots[0]!, ['unsettle', '--current']);
    expect(insideUnsettle).not.toContain('? [y/N]');
    expect((await snapshot(page)).sessions.find(s => s.id === candidate.id)?.settledAt).toBeNull();
    for (const action of ['settle', 'unsettle', 'settle'] as const) {
      const result = await helperPipe([action, candidate.id]);
      expect(result.code).toBe(0);
      expect(result.output).not.toContain('? [y/N]');
      expect((await snapshot(page)).sessions.find(s => s.id === candidate.id)?.settledAt === null).toBe(action === 'unsettle');
      expect(identity((await live(app)).filter(t => t.sessionId === candidate.id))).toEqual(candidateIdentity);
    }
    record('settle and unsettle preserve both native roots and inherited CLI context', { sessions: (await snapshot(page)).sessions, roots: await live(app) });
    const retired = await helperPipe(['archive', candidate.id]);
    expect(retired.code).not.toBe(0);
    expect(identity((await live(app)).filter(t => t.sessionId === candidate.id))).toEqual(candidateIdentity);
    const deletedSettled = await tty(data, ['delete', candidate.id], dir, 'y');
    record('external real PTY confirms deletion of a settled session with live tabs', deletedSettled);
    expect(deletedSettled.answered).toBe(true); expect(deletedSettled.code).toBe(0); expect(deletedSettled.output).toContain('Session deleted.');
    expect(await app.evaluate((_electron, id) => (globalThis as any).__shellfoxTest.repository.session(id) ?? null, candidate.id)).toBeNull();
    expect((await live(app)).some(t => t.sessionId === candidate.id)).toBe(false);
    expect(identity((await live(app)).filter(t => t.sessionId === keep.id))).toEqual(keepIdentity);
    const doomed = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID(), title: 'Delete live context' }));
    value(await page.evaluate(input => window.shellfox.addTab(input), { sessionId: doomed.id }));
    const doomedRoots = (await live(app)).filter(t => t.sessionId === doomed.id);
    expect(doomedRoots).toHaveLength(2);
    await confirmInside(doomedRoots[0]!, 'delete');
    await expect.poll(() => app.evaluate((_electron, id) => (globalThis as any).__shellfoxTest.repository.session(id) ?? null, doomed.id), { timeout: 30000 }).toBeNull();
    expect((await live(app)).some(t => t.sessionId === doomed.id)).toBe(false);
    expect(await app.evaluate((_electron, id) => (globalThis as any).__shellfoxTest.repository.tabs(id), doomed.id)).toEqual([]);
    expect(identity((await live(app)).filter(t => t.sessionId === keep.id))).toEqual(keepIdentity);
    record('delete committed after invoking CLI socket disappeared', { deletedSessionId: doomed.id, remaining: await live(app) });
    const stale = await requestControl(data, { action: 'inspect', marker: doomedRoots[0]!.root.marker }).then(() => 'unexpected success', error => String(error.message));
    expect(stale).toContain('context'); record('stale marker rejected', stale);
    const finalList = await helperPipe(['ls']);
    expect(finalList.code).toBe(0); expect(finalList.output).toContain(keep.id); expect(finalList.output).not.toContain(candidate.id); expect(finalList.output).not.toContain(doomed.id);
  } finally {
    const file = path.join(dir, 'control-evidence.json');
    await writeFile(file, JSON.stringify(evidence, null, 2));
    console.log('Live control evidence:', file);
    await testInfo.attach('live-control-evidence', { path: file, contentType: 'application/json' });
    // This backend belongs exclusively to the random tmp userData checked above.
    await app.evaluate(async (_electron, expectedData) => {
      const fixture = (globalThis as any).__shellfoxTest;
      if (_electron.app.getPath('userData') !== expectedData) throw new Error('Refusing cleanup outside isolated test data.');
      await fixture.service.closeForUpdate();
    }, data).catch(() => {});
    await app.close();
  }
});
