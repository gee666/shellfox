import { expect, it } from 'vitest';
import path from 'node:path';
import { mkdir, mkdtemp, writeFile, copyFile, rm } from 'node:fs/promises';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { createServer, type Socket } from 'node:net';
import { randomBytes, randomUUID } from 'node:crypto';
import { build } from 'esbuild';
import { WindowsCliIntegration } from './shellfox-cli';
import { WSL_STARTUP_SCRIPT } from './wsl-cli';
import { posixHelper } from './cli-helper';
import { publishControlEndpoint } from '../control-endpoint';
import { controlEnvelopeSchema } from '../control-protocol';
import type { ControlRequest } from '../control-types';

const exec = promisify(execFile);
// Explicit opt-in boots only the selected distro and changes only a project tmp fixture HOME.
it.skipIf(process.platform !== 'win32' || process.env.SHELLFOX_CLI_TEST_WSL !== '1')('installed WSL commands reach Windows loopback with Windows data and the existing marker', async () => {
  await mkdir(path.resolve('tmp'), { recursive: true });
  const root = await mkdtemp(path.resolve('tmp/wsl-control-launcher-'));
  const distro = process.env.SHELLFOX_CLI_TEST_WSL_DISTRO ?? 'Debian';
  const wsl = path.win32.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'wsl.exe');
  const guest = async (args: string[]) => (await exec(wsl, ['-d', distro, '--exec', ...args], { timeout: 20000, maxBuffer: 262144 })).stdout.trimEnd();
  const translate = (value: string) => guest(['wslpath', '-a', '-u', value]);
  const received: ControlRequest[] = [], secret = randomBytes(32).toString('hex'), marker = randomUUID();
  const sockets = new Set<Socket>();
  const server = createServer(socket => {
    sockets.add(socket); socket.on('close', () => sockets.delete(socket)); socket.on('error', () => {});
    let frame = '';
    socket.on('data', chunk => {
      frame += chunk.toString();
      if (!frame.includes('\n')) return;
      try {
        const envelope = controlEnvelopeSchema.parse(JSON.parse(frame.trim()));
        if (envelope.secret !== secret) throw new Error('Wrong manager endpoint');
        received.push(envelope.request);
        socket.end(JSON.stringify({ ok: true, response: { sessions: [], tabs: [], message: 'Windows manager reached.' } }) + '\n');
      } catch (error) { socket.end(JSON.stringify({ ok: false, error: (error as Error).message }) + '\n'); }
    });
  });
  let removeEndpoint: (() => Promise<void>) | undefined;
  try {
    await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
    const address = server.address();
    if (!address || typeof address === 'string') throw new Error('Missing fixture listener');
    const data = path.join(root, 'Windows manager data'), bin = path.join(root, 'Windows bin');
    const helper = path.join(root, 'cli', 'main', 'cli.cjs'), runtime = path.join(root, 'cli', 'cli-runtime', 'node.exe');
    await build({ bundle: true, platform: 'node', format: 'cjs', target: 'node24', external: ['cpu-features', '*.node'],
      entryPoints: ['src/main/platform/shellfox-cli-entry.ts'], outfile: helper, define: { __SHELLFOX_VERSION__: JSON.stringify('wsl-fixture') } });
    await mkdir(path.dirname(runtime), { recursive: true }); await copyFile(process.execPath, runtime);
    removeEndpoint = await publishControlEndpoint(data, { version: 1, host: '127.0.0.1', port: address.port, secret });
    const cli = new WindowsCliIntegration({ executable: process.execPath, cliHelper: helper, userData: data, binDir: bin,
      run: async () => ({ ok: true, value: { installed: true } }), wsl: false });
    expect(await cli.set(true)).toMatchObject({ ok: true, value: { installed: true } });
    const home = path.join(root, 'guest home'), oldBin = path.join(root, 'Linux bin');
    await mkdir(home); await mkdir(oldBin);
    await writeFile(path.join(oldBin, 'shellfox'), '#!/bin/sh\nprintf "%s\\n" "Wrong Linux launcher" >&2\nexit 79\n');
    const [guestHome, guestBin, guestOld] = await Promise.all([translate(home), translate(bin), translate(oldBin)]);
    const edited = JSON.parse(await guest(['python3', '-c', WSL_STARTUP_SCRIPT, 'enable', guestBin, guestHome]));
    expect(edited.errors).toEqual([]);
    // Source the installed startup block, not an explicit path to a hand-picked shim.
    const invoke = `export PATH="$1:/usr/bin:/bin"
. "$2/.bashrc"
export SHELLFOX_TERMINAL_MARKER="$3"
export WSLENV='KEEP/p:SHELLFOX_TERMINAL_MARKER/w:shellfox_terminal_marker/p:NO_COLOR/w'
export NO_COLOR=
shift 3
shellfox "$@"`;
    for (const [args, action] of [
      [['ls', '--current'], 'list'], [['stats', '--current'], 'stats'], [['ps', '--current'], 'processes'],
      [['tab', 'new'], 'tab-open'], [['tab', 'rm'], 'tab-close'],
    ] as const) {
      await guest(['/bin/bash', '--noprofile', '--norc', '-c', invoke, 'fixture', guestOld, guestHome, marker, ...args]);
      expect(received.at(-1)).toEqual({ action, current: true, marker });
    }
    expect(received).toHaveLength(5);
    // Probe the same interop wrapper to verify flag removal, unrelated flags and runtime platform.
    const probe = path.join(root, 'cli', 'main', 'probe.cjs'), probeShim = path.join(bin, 'probe');
    await writeFile(probe, 'console.log(JSON.stringify({platform:process.platform,marker:process.env.SHELLFOX_TERMINAL_MARKER,wslenv:process.env.WSLENV,args:process.argv.slice(2)}))');
    await writeFile(probeShim, '#!/bin/sh\n' + posixHelper({ executable: process.execPath, cliHelper: probe, userData: data }, true));
    const probeInvoke = invoke.replace('shellfox "$@"', '/bin/sh "$1"');
    const info = JSON.parse(await guest(['/bin/bash', '--noprofile', '--norc', '-c', probeInvoke, 'fixture', guestOld, guestHome, marker, await translate(probeShim)]));
    expect(info).toEqual({ platform: 'win32', marker, wslenv: 'KEEP/p:NO_COLOR:SHELLFOX_TERMINAL_MARKER', args: ['--user-data', data] });
    // Broken interop/runtime must fail, not silently run a Linux helper or selected manager.
    await rm(runtime);
    await expect(guest(['/bin/bash', '--noprofile', '--norc', '-c', invoke, 'fixture', guestOld, guestHome, marker, 'ls', '--current'])).rejects.toMatchObject({ code: 127 });
    expect(received).toHaveLength(5);
  } finally {
    for (const socket of sockets) socket.destroy();
    await new Promise<void>(resolve => server.close(() => resolve()));
    await removeEndpoint?.();
    await rm(root, { recursive: true, force: true });
  }
}, 180000);
