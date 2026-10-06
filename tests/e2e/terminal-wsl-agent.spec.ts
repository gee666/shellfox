import { test, expect } from '@playwright/test';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { mkdir, writeFile } from 'node:fs/promises';
import { launch, scratch, snapshot, value } from '../fixtures/electron';

const run = promisify(execFile);
const quote = (s: string) => "'" + s.replaceAll("'", "'\\''") + "'";
test('second WSL tab trusts Node named pi and shows isolated real guest Pi as green', async () => {
  test.skip(process.platform !== 'win32');
  test.setTimeout(90000);
  let nativeNode = '', piLauncher = '';
  try {
    const listed = await run('wsl.exe', ['--list', '--running', '--quiet'], { timeout: 8000, windowsHide: true, encoding: 'buffer' });
    const running = listed.stdout.toString(listed.stdout.includes(0) ? 'utf16le' : 'utf8').replace(/^\uFEFF/, '').split(/\r?\n/).map(s => s.trim());
    test.skip(!running.includes('Debian'), 'Uses only an already-running Debian, never boots a stopped distro.');
    const probe = await run('wsl.exe', ['-d', 'Debian', '--exec', '/bin/sh', '-c', 'command -v pi || exit 1; for n in /usr/bin/node "$HOME"/.nvm/versions/node/v24*/bin/node; do if [ -x "$n" ]; then printf "%s\\n" "$n"; fi; done'], { timeout: 8000, windowsHide: true });
    const lines = probe.stdout.trim().split(/\r?\n/);
    piLauncher = lines[0] ?? ''; nativeNode = lines.length > 1 ? lines.at(-1) ?? '' : '';
  } catch { /* Debian or a guest Pi launcher is not installed. */ }
  test.skip(!nativeNode.startsWith('/') || !piLauncher.startsWith('/'), 'Requires an existing Pi launcher and native guest Node. Windows interop Node is not guest process evidence.');
  const dir = await scratch('wsl-pi');
  for (const name of ['home', 'agent', 'cache', 'config', 'work']) await mkdir(path.join(dir, name));
  const converted = await run('wsl.exe', ['-d', 'Debian', '--exec', 'wslpath', '-a', '-u', dir], { timeout: 8000, windowsHide: true });
  const guestDir = converted.stdout.trim();
  const { app, page } = await launch(path.join(dir, 'data'), 'real');
  try {
    const initial = await snapshot(page);
    value(await page.evaluate(settings => window.shellfox.saveSettings({ ...settings, shellExecutable: null, terminalProfileId: 'pwsh' }), initial.settings));
    const session = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID(), title: 'Guest Pi' }));
    const added = value(await page.evaluate(sessionId => window.shellfox.addTab({ sessionId, profileId: 'wsl:Debian' }), session.id));
    const tab = added.tabs[1], dot = page.locator(`#terminal-tab-${tab.id} .status-dot`);
    await page.locator(`#terminal-tab-${tab.id}`).click();
    const tabState = async () => (await snapshot(page)).sessions.find(s => s.id === session.id)!.tabs.find(t => t.id === tab.id)!;
    const bin = quote(path.posix.dirname(nativeNode));
    value(await page.evaluate(input => window.shellfox.writeTerminal!(input), { tabId: tab.id, generation: tab.generation!,
      data: `PATH=${bin}:"$PATH" command node -e "process.title='pi';console.log('NAMED_'+'READY');setTimeout(()=>console.log('NAMED_'+'DONE'),12000)"\r` }));
    const hasOutput = (marker: string) => app.evaluate((_electron, input) => {
      const replay = (globalThis as any).__shellfoxTest.backend.attach({ tabId: input.id });
      return replay.ok && replay.value.chunks.map((c: any) => c.data).join('').includes(input.marker);
    }, { id: tab.id, marker });
    await expect.poll(() => hasOutput('NAMED_READY'), { timeout: 12000 }).toBe(true);
    await expect.poll(async () => (await tabState()).agents, { timeout: 10000 }).toBe(1);
    expect(await tabState()).toMatchObject({ status: 'running', agents: 1 });
    await expect(dot).toHaveAttribute('aria-label', /Agent (working|idle)/);
    const named = await app.evaluate(async (_electron, id) => {
      const { backend, service } = (globalThis as any).__shellfoxTest, root = backend.get(id).root;
      const snapshots = await service.tracker.snapshotProvider([root]);
      return snapshots.flatMap((s: any) => s.processes).filter((p: any) => p.marker === root.marker && p.argv?.[0] === 'pi');
    }, tab.id);
    expect(named).toHaveLength(1);
    expect(named[0]).toMatchObject({ executable: nativeNode, accessible: true });
    expect(named[0].birth).toEqual(expect.any(String));
    await writeFile(path.join(dir, 'named-evidence.json'), JSON.stringify({ process: named[0], tab: await tabState() }, null, 2));
    await expect.poll(() => hasOutput('NAMED_DONE'), { timeout: 15000 }).toBe(true);
    await expect.poll(async () => (await tabState()).agents, { timeout: 10000 }).toBe(0);
    // Run the installed shim with isolated config, no provider credentials, no
    // sessions, tools or prompts. Only this test-owned terminal receives input.
    const isolatedEnv = ['HOME=' + guestDir + '/home', 'XDG_CONFIG_HOME=' + guestDir + '/config', 'XDG_CACHE_HOME=' + guestDir + '/cache',
      'PI_CODING_AGENT_DIR=' + guestDir + '/agent', 'PI_OFFLINE=1', 'PI_SKIP_VERSION_CHECK=1', 'PI_TELEMETRY=0',
      'PATH=' + path.posix.dirname(nativeNode) + ':/usr/bin:/bin', 'TERM=xterm-256color', 'COLORTERM=truecolor', 'npm_config_cache=' + guestDir + '/cache'];
    const flags = ['--offline', '--no-session', '--no-tools', '--no-extensions', '--no-mcp', '--no-skills', '--no-prompt-templates', '--no-themes', '--no-context-files', '--no-approve'];
    value(await page.evaluate(input => window.shellfox.writeTerminal!(input), { tabId: tab.id, generation: tab.generation!,
      data: `cd ${quote(guestDir + '/work')} && /usr/bin/env -i ${isolatedEnv.map(quote).join(' ')} SHELLFOX_TERMINAL_MARKER="$SHELLFOX_TERMINAL_MARKER" ${quote(piLauncher)} ${flags.join(' ')}\r` }));
    await expect.poll(async () => (await tabState()).status, { timeout: 15000 }).toBe('running');
    expect((await tabState()).agents).toBe(1);
    await expect(dot).toHaveAttribute('aria-label', /Agent (working|idle)/);
    await expect(dot).toHaveClass(/dot-(running|busy)/);
    const detected = (await snapshot(page)).sessions.find(s => s.id === session.id)!;
    expect(detected.tabs[0].agents).toBe(0);
    const realPi = await app.evaluate(async (_electron, id) => {
      const { backend, service } = (globalThis as any).__shellfoxTest, root = backend.get(id).root;
      const snapshots = await service.tracker.snapshotProvider([root]);
      return snapshots.flatMap((s: any) => s.processes).filter((p: any) => p.marker === root.marker && p.argv?.[0] === 'pi');
    }, tab.id);
    expect(realPi).toHaveLength(1);
    expect(realPi[0]).toMatchObject({ executable: nativeNode, accessible: true });
    expect(realPi[0].birth).toEqual(expect.any(String));
    await writeFile(path.join(dir, 'evidence.json'), JSON.stringify({ nativeNode, piLauncher, realPi, session: detected }, null, 2));
    console.log('WSL Pi evidence', path.join(dir, 'evidence.json'));
  } finally {
    await writeFile(path.join(dir, 'output.json'), JSON.stringify(await app.evaluate(() => {
      const backend = (globalThis as any).__shellfoxTest.backend;
      return backend.live().map((owned: any) => backend.attach({ tabId: owned.tabId }));
    }), null, 2));
    try {
      const owned = (await snapshot(page)).sessions.flatMap(s => s.tabs).filter(t => t.terminalKind === 'embedded' && t.lifecycle !== 'closed');
      for (const tab of owned) value(await page.evaluate(input => window.shellfox.closeTab!(input), { tabId: tab.id, generation: tab.generation! }));
    } finally { await app.close(); }
  }
});
