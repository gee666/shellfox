import { beforeAll, describe, expect, it } from 'vitest';
import { execFile } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { promisify } from 'node:util';
import { terminalEnvironment } from './environment';
import { decodeWsl } from './profiles';

// Opt in: these tests launch real shells and use only an already-running WSL
// distro. No profiles, dotfiles, global environment or machine settings change.
const enabled = process.env.SHELLFOX_NESTED_ENV_TEST === '1';
const run = promisify(execFile);
const psQuote = (s: string) => "'" + s.replaceAll("'", "''") + "'";
const shQuote = (s: string) => "'" + s.replaceAll("'", "'\\''") + "'";
const payload = 'literal C:\\folder with spaces; $() ` " \' = 雪🙂 : /';
const marker = 'test-owned-nested-env-marker';
const names = ['SHELLFOX_ENV_PAYLOAD', 'SHELLFOX_ENV_EMPTY', 'Mixed_Session_Key', 'TERM', 'COLORTERM', 'SHELLFOX_TERMINAL_MARKER'];
const expected = [payload, '', 'case stays', 'vt100', '24bit', marker];
const variables = names.slice(0, -1).map((name, i) => ({ name, value: expected[i] }));
const env = (wsl = false) => terminalEnvironment({ ...process.env, SHELLFOX_ENV_PAYLOAD: 'stale', NESTED_ENV_HOST_PRIVATE: 'host-only secret', WSLENV: 'SHELLFOX_ENV_PAYLOAD/pw:TERM/l:COLORTERM/u' }, variables, { windows: process.platform === 'win32', wsl, marker });
// The .NET Framework GetEnvironmentVariable API reports an empty value as
// null. Read the environment block instead so missing and empty stay distinct.
const psProbe = `$vars = [Environment]::GetEnvironmentVariables(); $values = @(${names.map(name => `$vars[${psQuote(name)}]`).join(';')}); [Console]::WriteLine([Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes((ConvertTo-Json -Compress -InputObject $values))))`;
const pythonProbe = `import os,json; assert 'NESTED_ENV_HOST_PRIVATE' not in os.environ; print(json.dumps([os.environ.get(n) for n in [${names.map(name => `'${name}'`).join(',')}]]))`;
const nestedGuest = 'exec /bin/sh -c ' + shQuote('exec python3 -c ' + shQuote(pythonProbe));
const psArgs = (script: string) => ['-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(script, 'utf16le').toString('base64')];
const psInvoke = (file: string, script: string) => '& ' + [file, ...psArgs(script)].map(psQuote).join(' ');
async function output(file: string, args: string[], environment = env()): Promise<string> {
  const result = await run(file, args, { env: environment, encoding: 'utf8', windowsHide: true, windowsVerbatimArguments: file === cmd, timeout: 15000, maxBuffer: 1024 * 1024 });
  return result.stdout.trim();
}
function checkWindows(stdout: string) { expect(JSON.parse(Buffer.from(stdout, 'base64').toString('utf8'))).toEqual(expected); }
function checkGuest(stdout: string) { expect(JSON.parse(stdout)).toEqual(expected); }
const system32 = path.win32.join(process.env.SystemRoot || 'C:\\Windows', 'System32');
const shells = [
  { id: 'pwsh', file: path.win32.join(process.env.ProgramFiles || 'C:\\Program Files', 'PowerShell', '7', 'pwsh.exe') },
  { id: 'windows-powershell', file: path.win32.join(system32, 'WindowsPowerShell', 'v1.0', 'powershell.exe') },
];
const cmd = path.win32.join(system32, 'cmd.exe');
const wsl = path.win32.join(system32, 'wsl.exe');

describe.runIf(enabled && process.platform === 'win32')('real nested Windows shell environments', () => {
  for (const parent of shells) {
    for (const child of shells) it.runIf(existsSync(parent.file) && existsSync(child.file))(`${parent.id} -> ${child.id}`, async () => {
      checkWindows(await output(parent.file, psArgs(psInvoke(child.file, psProbe))));
    }, 20000);
    it.runIf(existsSync(parent.file))(`${parent.id} -> cmd -> ${parent.id}`, async () => {
      const command = `"${parent.file}" ${psArgs(psProbe).join(' ')}`;
      checkWindows(await output(parent.file, psArgs('& ' + [cmd, '/d', '/s', '/c', command].map(psQuote).join(' '))));
    }, 20000);
    it.runIf(existsSync(parent.file))(`cmd -> ${parent.id}`, async () => {
      checkWindows(await output(cmd, ['/d', '/s', '/c', `""${parent.file}" ${psArgs(psProbe).join(' ')}"`]));
    }, 20000);
  }
});

describe.runIf(enabled && process.platform === 'win32' && existsSync(wsl))('real nested WSL environments', () => {
  let distro: string;
  let guestPs: string;
  const guestArgs = (args: string[]) => ['--distribution', distro, '--exec', ...args];
  const wslInvoke = (args: string[]) => '& ' + [wsl, ...args].map(psQuote).join(' ');
  beforeAll(async () => {
    const listed = await run(wsl, ['--list', '--running', '--quiet'], { encoding: 'buffer', timeout: 8000, windowsHide: true, maxBuffer: 1024 * 1024 });
    const distros = decodeWsl(listed.stdout).trim().split(/\r?\n/).filter(Boolean);
    distro = process.env.SHELLFOX_NESTED_ENV_DISTRO ?? distros[0];
    if (!distro || !distros.includes(distro)) throw new Error('Opt-in env acceptance requires an already-running WSL distro.');
    expect(await output(wsl, guestArgs(['/bin/sh', '-c', 'test -x /bin/bash && command -v python3 >/dev/null && printf ready']))).toBe('ready');
    guestPs = existsSync(shells[0].file) ? await output(wsl, guestArgs(['wslpath', '-a', '-u', shells[0].file])) : '';
  }, 20000);
  for (const parent of shells) {
    it.runIf(existsSync(parent.file))(`${parent.id} -> wsl --exec -> Python`, async () => {
      checkGuest(await output(parent.file, psArgs(wslInvoke(guestArgs(['python3', '-c', pythonProbe])))));
    }, 20000);
    it.runIf(existsSync(parent.file))(`${parent.id} -> wsl default shell -> Python`, async () => {
      checkGuest(await output(parent.file, psArgs(wslInvoke(['--distribution', distro, '--', 'python3', '-c', pythonProbe]))));
    }, 20000);
    it.runIf(existsSync(parent.file))(`${parent.id} -> wsl -> Bash -> sh -> Python`, async () => {
      checkGuest(await output(parent.file, psArgs(wslInvoke(guestArgs(['/bin/bash', '--noprofile', '--norc', '-c', nestedGuest])))));
    }, 20000);
  }
  it('cmd -> wsl -> Python', async context => {
    // wsl.exe's cmd command-line parser treats quoted distro names differently
    // from PowerShell. Keep this fixture to simple names; other paths use argv.
    context.skip(!/^[A-Za-z0-9_.-]+$/.test(distro), 'cmd fixture requires a simple distro name');
    // Base64 keeps cmd and WSL quoting out of the Python fixture itself.
    const encoded = Buffer.from(pythonProbe).toString('base64');
    const script = `import base64;exec(base64.b64decode('${encoded}'))`;
    checkGuest(await output(cmd, ['/d', '/s', '/c', `""${wsl}" --distribution ${distro} --exec python3 -c "${script}""`]));
  }, 20000);
  it.runIf(existsSync(shells[0].file))('reproduces missing session values when only the marker is listed in WSLENV', async () => {
    const environment = env();
    environment.WSLENV = 'SHELLFOX_TERMINAL_MARKER';
    const result = JSON.parse(await output(shells[0].file, psArgs(wslInvoke(guestArgs(['python3', '-c', pythonProbe]))), environment));
    expect(result[0]).toBeNull();
    expect(result.at(-1)).toBe(marker);
  }, 20000);
  it('direct WSL -> Bash -> sh -> Python', async () => {
    checkGuest(await output(wsl, guestArgs(['/bin/bash', '--noprofile', '--norc', '-c', nestedGuest]), env(true)));
  }, 20000);
  it('native Unix env -> guest Bash -> sh, without WSLENV', async () => {
    const environment = terminalEnvironment({ PATH: '/usr/bin:/bin' }, variables, { windows: false, wsl: false, marker });
    expect(environment.WSLENV).toBeUndefined();
    const fixture = `import json,subprocess,sys; print(subprocess.check_output(['/bin/bash','--noprofile','--norc','-c',sys.argv[1]],env=json.loads(sys.argv[2]),text=True),end='')`;
    checkGuest(await output(wsl, guestArgs(['python3', '-c', fixture, nestedGuest, JSON.stringify(environment)])));
  }, 20000);
  it.runIf(existsSync(shells[0].file))('direct WSL -> pwsh -> WSL -> Python', async () => {
    const command = psArgs(wslInvoke(guestArgs(['python3', '-c', pythonProbe])));
    checkGuest(await output(wsl, guestArgs(['/bin/bash', '--noprofile', '--norc', '-c', 'exec "$@"', 'env-probe', guestPs, ...command]), env(true)));
  }, 20000);
});

describe.runIf(enabled && (process.platform === 'linux' || process.platform === 'darwin'))('real native Unix nested environments', () => {
  const probe = `printf '%s\\n' ${names.map(name => `"$${name}"`).join(' ')}`;
  for (const shell of ['/bin/bash', '/bin/zsh', '/bin/sh']) it.runIf(existsSync(shell))(`${shell} -> sh`, async () => {
    const flags = shell.endsWith('bash') ? ['--noprofile', '--norc'] : shell.endsWith('zsh') ? ['-f'] : [];
    expect(await output(shell, [...flags, '-c', 'exec /bin/sh -c ' + shQuote(probe)])).toBe(expected.join('\n'));
  }, 20000);
});
