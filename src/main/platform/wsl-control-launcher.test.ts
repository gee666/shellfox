import { expect, it, vi } from 'vitest';
import path from 'node:path';
import { mkdir, mkdtemp, writeFile, copyFile, rm } from 'node:fs/promises';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { randomUUID } from 'node:crypto';
import { build } from 'esbuild';
import { WindowsCliIntegration } from './shellfox-cli';
import { WSL_STARTUP_SCRIPT } from './wsl-cli';
import { posixHelper } from './cli-helper';
import { EmbeddedSessionService } from '../terminal/service';
import { PtyBackend } from '../terminal/backend';
import { factoryFixture, MemoryRepository } from '../terminal/test-fixtures';
import { startControlServer } from '../control-server';
import { startHostRpc } from '../../host/rpc';
const exec = promisify(execFile);
// Explicit opt-in boots only the selected distro and changes only a project tmp fixture HOME.
it.skipIf(process.platform !== 'win32' || process.env.SHELLFOX_CLI_TEST_WSL !== '1')('installed WSL namespaces reach Windows RPC and resolve only the Windows host marker', async () => {
  await mkdir(path.resolve('tmp'), { recursive: true });
  const root = await mkdtemp(path.resolve('tmp/wsl-control-launcher-'));
  const distro = process.env.SHELLFOX_CLI_TEST_WSL_DISTRO ?? 'Debian';
  const wsl = path.win32.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'wsl.exe');
  const guest = async (args: string[]) => (await exec(wsl, ['-d', distro, '--exec', ...args], { timeout: 20000, maxBuffer: 262144 })).stdout.trimEnd();
  const translate = (value: string) => guest(['wslpath', '-a', '-u', value]);
  const fixture = factoryFixture(), service = new EmbeddedSessionService(new MemoryRepository(), new PtyBackend(fixture.options), () => ({ setWatch() {}, dispose() {} }));
  await service.initialize();
  const created = await service.createSession({ cwd: process.cwd(), requestId: randomUUID() });
  if (!created.ok) throw new Error(created.error.message);
  const marker = service.backend.live()[0].root.marker;
  const context = vi.spyOn(service, 'controlSession');
  const data = path.join(root, 'Windows manager data'), bin = path.join(root, 'Windows bin');
  const rpc = await startHostRpc({ userData: data, service, mode: 'headless', shutdown: async () => {} });
  const control = await startControlServer(data, service, { active: false, set: active => active });
  try {
    const helper = path.join(root, 'cli', 'main', 'cli.cjs'), runtime = path.join(root, 'cli', 'cli-runtime', 'node.exe');
    await build({ bundle: true, platform: 'node', format: 'cjs', target: 'node24', external: ['cpu-features', '*.node'], loader: { '.sh': 'text' },
      entryPoints: ['src/main/platform/shellfox-cli-entry.ts'], outfile: helper, define: { __SHELLFOX_VERSION__: JSON.stringify('wsl-fixture') } });
    await mkdir(path.dirname(runtime), { recursive: true }); await copyFile(process.execPath, runtime);
    const cli = new WindowsCliIntegration({ executable: process.execPath, cliHelper: helper, userData: data, binDir: bin,
      run: async () => ({ ok: true, value: { installed: true } }), wsl: false });
    expect(await cli.set(true)).toMatchObject({ ok: true, value: { installed: true } });
    const home = path.join(root, 'guest home'), oldBin = path.join(root, 'Linux bin'); await mkdir(home); await mkdir(oldBin);
    await writeFile(path.join(oldBin, 'shellfox'), '#!/bin/sh\nprintf "%s\\n" "Wrong Linux launcher" >&2\nexit 79\n');
    const [guestHome, guestBin, guestOld] = await Promise.all([translate(home), translate(bin), translate(oldBin)]);
    expect(JSON.parse(await guest(['python3', '-c', WSL_STARTUP_SCRIPT, 'enable', guestBin, guestHome])).errors).toEqual([]);
    const invoke = `export PATH="$1:/usr/bin:/bin"
. "$2/.bashrc"
export SHELLFOX_TERMINAL_MARKER="$3"
export WSLENV='KEEP/p:SHELLFOX_TERMINAL_MARKER/w:shellfox_terminal_marker/p:NO_COLOR/w'
export NO_COLOR=
shift 3
shellfox "$@"`;
    const run = (args: string[]) => guest(['/bin/bash', '--noprofile', '--norc', '-c', invoke, 'fixture', guestOld, guestHome, marker, ...args]);
    for (const args of [['session', 'list', '--current'], ['ss', 'stats'], ['sessions', 'processes']]) {
      await run(args); expect(context.mock.calls.some(([request]) => request.action === 'inspect' && request.marker === marker)).toBe(true);
    }
    const added = JSON.parse(await run(['tm', 'create', '--json'])); expect(added.ok).toBe(true); expect(added.value.id).toBe(created.value.id);
    const tab = added.value.tabs.at(-1); await run(['terminals', 'close', tab.id, '--yes']);
    expect(fixture.processes[1].kill).toHaveBeenCalledOnce(); expect(fixture.processes[0].kill).not.toHaveBeenCalled();
    const probe = path.join(root, 'cli', 'main', 'probe.cjs'), probeShim = path.join(bin, 'probe');
    await writeFile(probe, 'console.log(JSON.stringify({platform:process.platform,marker:process.env.SHELLFOX_TERMINAL_MARKER,wslenv:process.env.WSLENV,args:process.argv.slice(2)}))');
    await writeFile(probeShim, '#!/bin/sh\n' + posixHelper({ executable: process.execPath, cliHelper: probe, userData: data }, true));
    const info = JSON.parse(await guest(['/bin/bash', '--noprofile', '--norc', '-c', invoke.replace('shellfox "$@"', '/bin/sh "$1"'), 'fixture', guestOld, guestHome, marker, await translate(probeShim)]));
    expect(info).toEqual({ platform: 'win32', marker, wslenv: 'KEEP/p:NO_COLOR:SHELLFOX_TERMINAL_MARKER', args: ['--user-data', data] });
    const calls = context.mock.calls.length; await rm(runtime);
    await expect(run(['session', 'list', '--current'])).rejects.toMatchObject({ code: 127 }); expect(context).toHaveBeenCalledTimes(calls);
  } finally { context.mockRestore(); await rpc.close(); await control.dispose(); await service.dispose(); await rm(root, { recursive: true, force: true }); }
}, 180000);
