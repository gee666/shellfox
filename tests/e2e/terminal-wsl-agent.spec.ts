import { test, expect } from '@playwright/test';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { writeFile } from 'node:fs/promises';
import { launch, scratch, snapshot, value } from '../fixtures/electron';

const run = promisify(execFile);
test('second WSL tab rejects ordinary Node titled pi and detects verified native guest Pi', async () => {
  test.skip(process.platform !== 'win32');
  test.setTimeout(60000);
  let nativeNode = '';
  try {
    const probe = await run('wsl.exe', ['-d', 'Debian', '--exec', '/bin/sh', '-c', 'command -v pi >/dev/null || exit 1; for n in /usr/bin/node "$HOME"/.nvm/versions/node/v24*/bin/node; do if [ -x "$n" ]; then printf "%s\\n" "$n"; fi; done'], { timeout: 8000, windowsHide: true });
    nativeNode = probe.stdout.trim().split(/\r?\n/).at(-1) ?? '';
  } catch { /* Debian or a guest Pi launcher is not installed. */ }
  test.skip(!nativeNode.startsWith('/'), 'Requires Debian, a Pi launcher and native guest Node 24. Windows interop Node is not guest process evidence.');
  const dir = await scratch('wsl-pi'), { app, page } = await launch(path.join(dir, 'data'), 'real');
  try {
    const initial = await snapshot(page);
    value(await page.evaluate(settings => window.shellfox.saveSettings({ ...settings, shellExecutable: null, terminalProfileId: 'pwsh' }), initial.settings));
    const session = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID(), title: 'Guest Pi' }));
    const added = value(await page.evaluate(sessionId => window.shellfox.addTab({ sessionId, profileId: 'wsl:Debian' }), session.id));
    const tab = added.tabs[1];
    // The existing pnpm shim chooses native guest Node when it is on PATH.
    // This changes only the test shell invocation, never guest/user settings.
    const bin = path.posix.dirname(nativeNode).replace(/'/g, "'\\''");
    value(await page.evaluate(input => window.shellfox.writeTerminal!(input), { tabId: tab.id, generation: tab.generation!,
      data: `PATH='${bin}':"$PATH" command node -e "process.title='pi';console.log('SPOOF_'+'READY');setTimeout(()=>console.log('SPOOF_'+'DONE'),4000)"\r` }));
    const hasOutput = (marker: string) => app.evaluate((_electron, input) => {
      const replay = (globalThis as any).__shellfoxTest.backend.attach({ tabId: input.id });
      return replay.ok && replay.value.chunks.map((c: any) => c.data).join('').includes(input.marker);
    }, { id: tab.id, marker });
    await expect.poll(() => hasOutput('SPOOF_READY'), { timeout: 12000 }).toBe(true);
    const spoof = await app.evaluate(async (_electron, id) => {
      const { backend, service } = (globalThis as any).__shellfoxTest;
      await service.tracker.poll();
      const root = backend.get(id).root;
      const snapshots = await service.tracker.snapshotProvider([root]);
      return snapshots.flatMap((s: any) => s.processes).filter((p: any) => p.marker === root.marker && p.argv?.[0] === 'pi');
    }, tab.id);
    expect(spoof).toHaveLength(1);
    expect(spoof[0].launchScript).toBeUndefined();
    const rejected = (await snapshot(page)).sessions.find(s => s.id === session.id)!.tabs[1];
    expect(rejected).toMatchObject({ status: 'waiting', agents: 0 });
    await writeFile(path.join(dir, 'spoof-evidence.json'), JSON.stringify({ process: spoof[0], tab: rejected }, null, 2));
    await expect.poll(() => hasOutput('SPOOF_DONE'), { timeout: 6000 }).toBe(true);
    // The ordinary Node child exits naturally before real Pi starts.
    value(await page.evaluate(input => window.shellfox.writeTerminal!(input), { tabId: tab.id, generation: tab.generation!, data: `PATH='${bin}':"$PATH" command pi\r` }));
    await expect.poll(async () => (await snapshot(page)).sessions.find(s => s.id === session.id)!.tabs[1].status, { timeout: 15000 }).toBe('running');
    const detected = (await snapshot(page)).sessions.find(s => s.id === session.id)!;
    expect(detected.tabs[1].agents).toBeGreaterThan(0);
    expect(detected.tabs[0].agents).toBe(0);
    const verified = await app.evaluate(async (_electron, id) => {
      const { backend, service } = (globalThis as any).__shellfoxTest, root = backend.get(id).root;
      const snapshots = await service.tracker.snapshotProvider([root]);
      return snapshots.flatMap((s: any) => s.processes).filter((p: any) => p.marker === root.marker && p.argv?.[0] === 'pi');
    }, tab.id);
    expect(verified.some((p: any) => /\/pi-coding-agent\/dist\/(bundle\/)?cli\.js$/.test(p.launchScript ?? ''))).toBe(true);
    await writeFile(path.join(dir, 'evidence.json'), JSON.stringify({ nativeNode, session: detected, roots: await app.evaluate(() => (globalThis as any).__shellfoxTest.backend.live()) }, null, 2));
    console.log('WSL Pi evidence', path.join(dir, 'evidence.json'));
  } finally {
    await writeFile(path.join(dir, 'output.json'), JSON.stringify(await app.evaluate(() => {
      const backend = (globalThis as any).__shellfoxTest.backend;
      return backend.live().map((owned: any) => backend.attach({ tabId: owned.tabId }));
    }), null, 2));
    await writeFile(path.join(dir, 'processes.json'), JSON.stringify(await app.evaluate(async () => {
      const { backend, service } = (globalThis as any).__shellfoxTest;
      const roots = backend.live().map((e: any) => e.root);
      const snapshots = await service.tracker.snapshotProvider(roots);
      return snapshots.map((s: any) => ({ ...s, processes: s.processes.filter((p: any) => roots.some((r: any) => p.marker === r.marker || p.pid === r.pid && s.environment === r.environment)) }));
    }), null, 2));
    console.log('WSL output', path.join(dir, 'output.json'));
    await page.evaluate(async () => { const result = await window.shellfox.getSnapshot(); if (result.ok) for (const s of result.value.sessions) for (const t of s.tabs) if (t.terminalKind === 'embedded' && t.lifecycle !== 'closed') await window.shellfox.closeTab!({ tabId: t.id, generation: t.generation! }); }).catch(() => {});
    await app.close();
  }
});
