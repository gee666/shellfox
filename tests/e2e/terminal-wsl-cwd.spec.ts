import { test, expect, type ElectronApplication, type Page } from '@playwright/test';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { createRequire } from 'node:module';
import { randomUUID, createHash } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import type { TabDto } from '../../src/shared/contracts';
import { shellfoxShims } from '../../src/main/platform/shellfox-cli';
import { setWslCli, runWslCli, type WslCliRunner } from '../../src/main/platform/wsl-cli';
import { launch, scratch, snapshot, value, root, testMain } from '../fixtures/electron';

const exec = promisify(execFile);
const wsl = path.win32.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'wsl.exe');
async function guest(distro: string, args: string[]) {
  return (await exec(wsl, ['--distribution', distro, '--exec', ...args], { timeout: 15000, windowsHide: true, maxBuffer: 65536 })).stdout.replace(/\r/g, '').replace(/\n$/, '');
}
async function output(app: ElectronApplication, tabId: string) {
  return app.evaluate((_electron, id) => {
    const result = (globalThis as any).__shellfoxTest.backend.attach({ tabId: id });
    if (!result.ok) throw new Error(JSON.stringify(result));
    return result.value.chunks.map((chunk: { data: string }) => chunk.data).join('');
  }, tabId);
}
async function shellCwd(app: ElectronApplication, page: Page, tab: TabDto, environment: 'local' | 'wsl') {
  expect(tab.lifecycle, JSON.stringify(tab)).toBe('open');
  const token = randomUUID().replace(/-/g, '');
  // Encode the result to keep spaces, quotes and terminal control sequences out of the assertion.
  // The output marker never appears verbatim in the echoed input.
  const command = environment === 'local'
    ? `[Console]::WriteLine(('CWD_'+'${token}='+[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes((Get-Location).Path))+'_'+ 'END'))`
    : `printf 'CWD_%s=%s_%s\\n' '${token}' "$(printf '%s' "$PWD" | base64 -w0)" END`;
  value(await page.evaluate(input => window.shellfox.writeTerminal!(input), { tabId: tab.id, generation: tab.generation!, data: command + '\r' }));
  const match = new RegExp(`CWD_${token}=([A-Za-z0-9+/=]+)_END`);
  let encoded = '';
  await expect.poll(async () => {
    const clean = (await output(app, tab.id)).replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, '').replace(/\r?\n/g, '');
    encoded = match.exec(clean)?.[1] ?? '';
    return !!encoded;
  }, { timeout: 15000 }).toBe(true);
  return Buffer.from(encoded, 'base64').toString('utf8');
}

for (const kind of ['mounted drive', 'guest filesystem'] as const) {
  test(`real Windows and WSL shells start in the ${kind} session cwd`, async () => {
    test.skip(process.platform !== 'win32', 'Windows WSL cwd transport.');
    test.setTimeout(180000);
    let distro = '';
    try {
      const result = await exec(wsl, ['--list', '--quiet'], { timeout: 15000, windowsHide: true, encoding: 'buffer' });
      const names = (result.stdout.includes(0) ? result.stdout.toString('utf16le') : result.stdout.toString('utf8')).replace(/^\uFEFF/, '').split(/\r?\n/).map(s => s.trim()).filter(Boolean);
      distro = names.includes('Debian') ? 'Debian' : names[0] ?? '';
    } catch { /* No WSL installation. */ }
    test.skip(!distro, 'Requires an installed WSL distribution.');
    const dir = await scratch('terminal-wsl-cwd');
    const data = path.join(dir, 'data'), bin = path.join(dir, 'Windows shim bin');
    const home = path.join(dir, 'isolated guest home'), oldBin = path.join(dir, 'old Linux bin');
    const { app, page } = await launch(data, 'real');
    let guestCwd = '', hostCwd = '', guestHome = '';
    const evidence: unknown[] = [], startupEvidence: unknown[] = [];
    // Only enumeration is faked. All edits execute the real Python startup editor in this distro.
    const startupRun: WslCliRunner = async (file, args) => {
      if (args.includes('--list')) return Buffer.from(distro + '\r\n', 'utf16le');
      expect(args.slice(0, 3)).toEqual(['--distribution', distro, '--exec']);
      if (args.includes('python3')) expect(args.at(-1), 'Never edit the real guest HOME').toBe(guestHome);
      else expect(args[3]).toBe('wslpath');
      return runWslCli(file, args);
    };
    const startupNames = ['.bashrc', '.bash_profile', '.bash_login', '.profile', '.zshrc', '.zprofile', '.zlogin'];
    const startupBytes = async () => Object.fromEntries(await Promise.all(startupNames.map(async name => {
      const bytes = await readFile(path.join(home, name)).catch(error => { if (error.code === 'ENOENT') return null; throw error; });
      return [name, bytes?.toString('base64') ?? null];
    })));
    const appState = () => app.evaluate(({ BrowserWindow, Menu }) => ({
      pid: process.pid, windows: BrowserWindow.getAllWindows().map(window => ({ id: window.id, menuBarVisible: window.isMenuBarVisible() })),
      menu: Menu.getApplicationMenu() === null ? null : 'present',
    }));
    try {
      const initial = await snapshot(page);
      const originalApp = await appState();
      expect.soft(originalApp.menu, 'Windows application menu must be removed').toBeNull();
      expect(originalApp.windows).toHaveLength(1);
      expect.soft(originalApp.windows[0].menuBarVisible, 'Windows BrowserWindow must have no visible menu').toBe(false);
      const profiles = value(await page.evaluate(() => window.shellfox.getTerminalProfiles!())).profiles;
      const guestProfile = profiles.find(p => p.id === `wsl:${distro}`);
      test.skip(!guestProfile?.available, guestProfile?.unavailableReason ?? 'Guest safe-close preflight is unavailable.');
      const locals = profiles.filter(p => p.environment === 'local' && p.available && ['pwsh', 'windows-powershell'].includes(p.id));
      test.skip(!locals.length, 'Requires a discovered PowerShell.');
      if (kind === 'mounted drive') {
        const mounted = path.join(dir, "mounted space & 'quote' % ! 雪");
        await mkdir(mounted);
        guestCwd = await guest(distro, ['wslpath', '-u', mounted]);
      } else {
        const home = await guest(distro, ['/bin/sh', '-c', 'printf "%s" "$HOME"']);
        guestCwd = `${home}/shellfox-cwd-${randomUUID()} space & 'quote' % ! 雪`;
        await guest(distro, ['mkdir', '--', guestCwd]);
      }
      // This is the exact path representation forwarded by a WSL shellfox start caller.
      hostCwd = await guest(distro, ['wslpath', '-w', guestCwd]);
      await mkdir(bin);
      const shims = shellfoxShims({ executable: createRequire(path.resolve('package.json'))('electron') as string, appPath: testMain,
        prefixArgs: ['--test-user-data', data, '--test-backend', 'real'], launchEnv: { SHELLFOX_TEST_MODE: '1', SHELLFOX_TEST_ROOT: root } });
      await writeFile(path.join(bin, 'shellfox'), shims.posix);
      await writeFile(path.join(bin, 'shellfox-dispatch.ps1'), shims.dispatcher);
      const shim = await guest(distro, ['wslpath', '-u', path.join(bin, 'shellfox')]);
      await mkdir(home); await mkdir(oldBin);
      guestHome = await guest(distro, ['wslpath', '-u', home]);
      const guestOldBin = await guest(distro, ['wslpath', '-u', oldBin]);
      const oldLauncher = guestOldBin + '/shellfox';
      // Preserve mixed line endings and no final newline, not just equivalent shell configuration.
      await writeFile(path.join(home, '.bashrc'), Buffer.from('# isolated user bash config\r\n# no final newline'));
      await writeFile(path.join(home, '.profile'), Buffer.from('# isolated user profile\r\nexport SHELLFOX_CWD_TEST=unchanged'));
      await writeFile(path.join(oldBin, 'shellfox'), '#!/bin/sh\nprintf "OLD_SHELLFOX_0.1.1\\n"\n');
      await guest(distro, ['chmod', '+x', oldLauncher, shim]);
      const initialStartup = await startupBytes();
      const initialPath = `${guestOldBin}:${path.posix.dirname(shim)}:/usr/bin:/bin`;
      const bash = async (command: string) => {
        const result = await exec(wsl, ['--distribution', distro, '--cd', guestCwd, '--exec', '/usr/bin/env',
          `HOME=${guestHome}`, `PATH=${initialPath}`, '/bin/bash', '--noprofile', '-i', '-c', command],
          { timeout: 30000, windowsHide: true, maxBuffer: 65536 });
        return { stdout: result.stdout.replace(/\r/g, ''), stderr: result.stderr };
      };
      const probeCommand = 'printf "RESOLVED=%s\\n" "$(command -v shellfox)"; shellfox start .';
      const old = await bash(probeCommand);
      expect(old.stdout).toContain(`RESOLVED=${oldLauncher}\n`);
      expect(old.stdout).toContain('OLD_SHELLFOX_0.1.1');
      expect((await snapshot(page)).sessions).toHaveLength(0);
      expect(await appState()).toEqual(originalApp);
      const enabled = await setWslCli(true, bin, { home: guestHome, run: startupRun });
      expect(enabled).toContain('Restart WSL shells');
      const managedStartup = await startupBytes();
      expect(await readFile(path.join(home, '.bashrc'), 'utf8')).toContain('# >>> Shellfox/wsl-cli-v1 >>>');
      expect(await setWslCli(true, bin, { home: guestHome, run: startupRun })).toContain('Restart WSL shells');
      expect(await startupBytes()).toEqual(managedStartup);
      startupEvidence.push({ stage: 'before enable', ...old }, { stage: 'enabled', reason: enabled, initialPath, guestHome, initialStartup, managedStartup });
      const startFromWsl = async () => {
        const before = new Set((await snapshot(page)).sessions.map(s => s.id));
        // The caller exits immediately. Read cwd from the new shell, not from session metadata alone.
        const result = await bash(probeCommand);
        startupEvidence.push({ stage: 'bare interactive dispatch', ...result });
        expect(result.stdout).toContain(`RESOLVED=${shim}\n`);
        expect(result.stdout).not.toContain('OLD_SHELLFOX_0.1.1');
        expect(result.stdout).toContain('Shellfox: started session');
        await expect.poll(async () => (await snapshot(page)).sessions.filter(s => !before.has(s.id)).length, { timeout: 20000 }).toBe(1);
        const session = (await snapshot(page)).sessions.find(s => !before.has(s.id))!;
        expect(session.cwd.toLowerCase()).toBe(hostCwd.toLowerCase());
        expect(await appState()).toEqual(originalApp);
        return session;
      };
      const check = async (tab: TabDto, environment: 'local' | 'wsl') => {
        const actual = await shellCwd(app, page, tab, environment);
        const expected = environment === 'wsl' ? guestCwd : hostCwd;
        evidence.push({ kind, profileId: tab.profileId, tabCwd: tab.cwd, actual, expected, output: await output(app, tab.id) });
        expect(tab.cwd).toBe(expected);
        if (environment === 'wsl') expect(actual).toBe(guestCwd);
        else expect(actual.replace(/^Microsoft\.PowerShell\.Core\\FileSystem::/, '').toLowerCase()).toBe(hostCwd.toLowerCase());
      };
      const close = async (tabs: TabDto[]) => {
        for (const tab of tabs) value(await page.evaluate(input => window.shellfox.closeTab!(input), { tabId: tab.id, generation: tab.generation! }));
      };
      for (const local of locals) {
        value(await page.evaluate(settings => window.shellfox.saveSettings(settings), { ...initial.settings, terminalProfileId: local.id, shellExecutable: null }));
        const session = await startFromWsl();
        await check(session.tabs[0], 'local');
        const mixed = value(await page.evaluate(input => window.shellfox.addTab(input), { sessionId: session.id, profileId: guestProfile!.id }));
        expect(mixed.cwd).toBe(hostCwd);
        await check(mixed.tabs[1], 'wsl');
        await close(mixed.tabs);
        const reopened = value(await page.evaluate(input => window.shellfox.activateSession(input), { sessionId: session.id }));
        await check(reopened.tabs.at(-1)!, 'local');
        await close([reopened.tabs.at(-1)!]);
      }
      value(await page.evaluate(settings => window.shellfox.saveSettings(settings), { ...initial.settings, terminalProfileId: guestProfile!.id, shellExecutable: null }));
      const session = await startFromWsl();
      await check(session.tabs[0], 'wsl');
      const mixed = value(await page.evaluate(input => window.shellfox.addTab(input), { sessionId: session.id, profileId: locals[0].id }));
      expect(mixed.cwd).toBe(hostCwd);
      await check(mixed.tabs[1], 'local');
      await close(mixed.tabs);
      const beforeDisable = (await snapshot(page)).sessions.map(s => s.id);
      const disabled = await setWslCli(false, bin, { home: guestHome, run: startupRun });
      expect(disabled).toContain('Restart WSL shells');
      expect(await startupBytes()).toEqual(initialStartup);
      const restored = await bash(probeCommand);
      expect(restored.stdout).toContain(`RESOLVED=${oldLauncher}\n`);
      expect(restored.stdout).toContain('OLD_SHELLFOX_0.1.1');
      expect(restored.stdout).not.toContain('Shellfox: started session');
      expect((await snapshot(page)).sessions.map(s => s.id)).toEqual(beforeDisable);
      expect(await appState()).toEqual(originalApp);
      startupEvidence.push({ stage: 'disabled', reason: disabled, ...restored, restoredStartup: await startupBytes(), app: await appState() });
    } finally {
      try {
        const evidenceFile = path.join(dir, 'evidence.json');
        await writeFile(evidenceFile, JSON.stringify({ distro, guestCwd, hostCwd, evidence, startupEvidence, sessions: (await snapshot(page)).sessions,
          output: await app.evaluate(() => { const backend = (globalThis as any).__shellfoxTest.backend; return backend.live().map((t: any) => backend.attach({ tabId: t.tabId })); }) }, null, 2));
        await test.info().attach('terminal-cwd-evidence', { path: evidenceFile, contentType: 'application/json' });
        console.log('Terminal cwd evidence', evidenceFile);
      } finally {
        try {
          await page.evaluate(async () => { const result = await window.shellfox.getSnapshot(); if (result.ok) for (const s of result.value.sessions) for (const t of s.tabs) if (t.terminalKind === 'embedded' && t.lifecycle !== 'closed') await window.shellfox.closeTab!({ tabId: t.id, generation: t.generation! }); }).catch(() => {});
          await app.close();
        } finally {
          if (guestHome) await setWslCli(false, bin, { home: guestHome, run: startupRun });
          const key = 'Software\\Shellfox\\Tests\\' + createHash('sha256').update(data).digest('hex');
          const script = `[Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree('${key}', $false)`;
          await exec('powershell.exe', ['-NoProfile', '-EncodedCommand', Buffer.from(script, 'utf16le').toString('base64')], { timeout: 15000, windowsHide: true }).catch(() => {});
          if (kind === 'guest filesystem' && guestCwd) await guest(distro, ['rmdir', '--', guestCwd]);
        }
      }
    }
  });
}
