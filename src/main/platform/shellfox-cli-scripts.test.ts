import { afterEach, expect, it } from 'vitest';
import path from 'node:path';
import { access, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { execFile, spawn } from 'node:child_process';
import { promisify } from 'node:util';
import { build } from 'esbuild';
import { shellfoxShims } from './shellfox-cli';
import { linuxLauncher } from './linux-cli';
import { findGitBash, scriptContext, quoteMsysArgument } from './shellfox-cli-scripts';
const exec = promisify(execFile);
const roots: string[] = [];
afterEach(async () => { await Promise.all(roots.splice(0).map(root => rm(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 }))); });
async function fixture() {
  await mkdir(path.resolve('tmp'), { recursive: true });
  const root = await mkdtemp(path.resolve('tmp/cli-execution-')); roots.push(root);
  const helper = path.join(root, 'main', 'cli.cjs'), data = path.join(root, 'user data');
  await build({ entryPoints: ['src/main/platform/shellfox-cli-entry.ts'], outfile: helper, bundle: true, platform: 'node', format: 'cjs', packages: 'external', define: { __SHELLFOX_VERSION__: '"test"' } });
  const run = async (args: string[]) => {
    try { return { ...(await exec(process.execPath, [helper, '--user-data', data, ...args], { cwd: root, timeout: 10000 })), code: 0 }; }
    catch (error) { const result = error as Error & { stdout: string; stderr: string; code: number }; return result; }
  };
  return { root, helper, data, run };
}
const literalArgs = ['', 'two words', "it's", '"quoted"', '\\', '$(touch injected)', '`touch injected`', ';touch injected', '&echo bad', '%PATH%', '!bang!', '--help', '-h', 'help', '--register', '--rename', '--remove', '--rm', '--unregister', '--list', '--ls', '--run', '-n', 'newline\nvalue'];
const gitBash = process.platform === 'win32' ? await findGitBash(scriptContext()).catch(() => undefined) : undefined;
const wslDistro = process.platform === 'win32' ? process.env.SHELLFOX_TEST_WSL_DISTRO : undefined;
const linuxNode = wslDistro ? process.env.SHELLFOX_TEST_LINUX_NODE : undefined;

it.skipIf(process.platform === 'win32' && !gitBash)('executes current Bash script contents with literal argv and exit codes, including path spaces and injection text', async () => {
  const { root, run } = await fixture(); const file = path.join(root, "it's a script & literal.sh");
  await writeFile(file, '#!/bin/bash\nprintf "%s\\0" "$@"\nexit 23\n');
  expect((await run(['cmd', '--register', 'showargs', file])).code).toBe(0);
  const first = await run(['cmd', 'showargs', ...literalArgs]);
  expect(first.code).toBe(23); expect(first.stdout.split('\0').slice(0, -1)).toEqual(literalArgs);
  await expect(access(path.join(root, 'injected'))).rejects.toThrow();
  await writeFile(file, 'printf "latest contents"\nexit 7\n');
  expect(await run(['cmd', '--run', 'SHOWARGS'])).toMatchObject({ code: 7, stdout: 'latest contents' });
  await rm(file); expect((await run(['cmd', 'showargs'])).code).not.toBe(0);
});
it.skipIf(process.platform !== 'win32')('executes PowerShell with literal quotes, empty arguments and injection text', async () => {
  const { root, run } = await fixture(); const file = path.join(root, "it's a script & literal.ps1");
  await writeFile(file, '[Console]::Write((ConvertTo-Json -Compress -InputObject @($args))); exit 19');
  expect((await run(['cmd', '--register', 'psargs', file])).code).toBe(0);
  const result = await run(['cmd', '--run', 'psargs', ...literalArgs]);
  expect(result.code, result.stderr).toBe(19); expect(JSON.parse(result.stdout)).toEqual(literalArgs);
  await expect(access(path.join(root, 'injected'))).rejects.toThrow();
});
it.skipIf(process.platform !== 'win32')('forwards safe cmd shell arguments without using them in batch conditions', async () => {
  const { root, helper, data } = await fixture();
  const shim = path.join(root, 'shellfox.cmd'), script = path.join(root, 'script space.ps1');
  await writeFile(script, '[Console]::Write((ConvertTo-Json -Compress -InputObject @($args))); exit 0');
  const runtime = path.win32.join(path.dirname(helper), '../cli-runtime/node.exe');
  await writeFile(shim, shellfoxShims({ executable: process.execPath, cliHelper: helper, userData: data }).cmd.replaceAll(runtime, process.execPath));
  const command = `""${shim}" cmd --register echoargs "${script}""`;
  await exec('cmd.exe', ['/d', '/s', '/c', command], { cwd: root, timeout: 10000, windowsVerbatimArguments: true });
  const result = await exec('cmd.exe', ['/d', '/s', '/c', `""${shim}" cmd echoargs "two words" "literal & echo hacked" "!bang!" "--help""`], { cwd: root, timeout: 10000, windowsVerbatimArguments: true });
  expect(JSON.parse(result.stdout)).toEqual(['two words', 'literal & echo hacked', '!bang!', '--help']);
});
it.skipIf(process.platform !== 'win32')('uses the PowerShell shim to preserve empty values, quotes and shell metacharacters, restoring caller environment', async () => {
  const { root, helper, data, run } = await fixture();
  const shim = path.join(root, 'shellfox.ps1'), script = path.join(root, 'argv.ps1');
  await writeFile(script, '[Console]::Write((ConvertTo-Json -Compress -InputObject @($args))); exit 0');
  const runtime = path.win32.join(path.dirname(helper), '../cli-runtime/node.exe');
  const shims = shellfoxShims({ executable: process.execPath, cliHelper: helper, userData: data });
  await writeFile(shim, shims.powershell.replaceAll(runtime, process.execPath));
  await writeFile(path.join(root, 'shellfox.cmd'), shims.cmd.replaceAll(runtime, process.execPath));
  expect((await run(['cmd', '--register', 'psshim', script])).code).toBe(0);
  const payload = Buffer.from(JSON.stringify(['cmd', 'psshim', ...literalArgs]), 'utf8').toString('base64');
  const code = `$env:Path='${root.replaceAll("'", "''")};'+$env:Path; if ((Get-Command shellfox).Source -ne '${shim.replaceAll("'", "''")}') { exit 92 }; $env:SHELLFOX_CLI_LAUNCH='original'; $env:ELECTRON_RUN_AS_NODE='original'; $argv=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('${payload}')) | ConvertFrom-Json; shellfox @argv; if ($env:SHELLFOX_CLI_LAUNCH -ne 'original' -or $env:ELECTRON_RUN_AS_NODE -ne 'original') { exit 91 }; exit $LASTEXITCODE`;
  const result = await exec('powershell.exe', ['-NoLogo', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', Buffer.from(code, 'utf16le').toString('base64')], { cwd: root, timeout: 10000 });
  expect(JSON.parse(result.stdout)).toEqual(literalArgs);
});
it.skipIf(process.platform !== 'win32' || !gitBash)('registers a Git Bash relative path from its caller directory and forwards POSIX argv unchanged', async () => {
  const { root, helper, data } = await fixture(); const shim = path.join(root, 'shellfox');
  const file = path.join(root, "it's a local script.sh"); await writeFile(file, 'printf "%s\\0" "$@"\n');
  const runtime = path.win32.join(path.dirname(helper), '../cli-runtime/node.exe');
  await writeFile(shim, shellfoxShims({ executable: process.execPath, cliHelper: helper, userData: data }).posix.replaceAll(runtime, process.execPath));
  const runGit = (args: string[]) => new Promise<{ stdout: string; stderr: string }>((resolve, reject) => {
    const child = spawn(gitBash!, args.map(quoteMsysArgument), { cwd: root, timeout: 10000, windowsVerbatimArguments: true, argv0: quoteMsysArgument(gitBash!), env: { ...process.env, MSYS_ARG_CONV_EXCL: '*' } });
    let stdout = '', stderr = '';
    child.stdout!.on('data', data => { stdout += data; }); child.stderr!.on('data', data => { stderr += data; });
    child.once('error', reject); child.once('close', code => code === 0 ? resolve({ stdout, stderr }) : reject(new Error(stderr)));
  });
  await runGit(['--', shim.replaceAll('\\', '/'), 'cmd', '--register', 'gitargs', path.basename(file)]);
  const registry = JSON.parse(await readFile(path.join(data, 'cli-commands.json'), 'utf8'));
  expect(registry.commands[0].path.toLowerCase()).toBe(file.toLowerCase());
  const apostropheFile = path.join(root, "it's.sh"); await writeFile(apostropheFile, 'printf "apostrophe path"');
  await runGit(['--', shim.replaceAll('\\', '/'), 'cmd', '--register', 'apostrophe', path.basename(apostropheFile)]);
  expect((await runGit(['--', shim.replaceAll('\\', '/'), 'cmd', '--run', 'apostrophe'])).stdout).toBe('apostrophe path');
  const result = await runGit(['--', shim.replaceAll('\\', '/'), 'cmd', 'gitargs', ...literalArgs]);
  expect(result.stdout.split('\0').slice(0, -1)).toEqual(literalArgs);
});
it.skipIf(!wslDistro)('registers a WSL reference with its distro and executes with literal argv from WSL and Windows', async () => {
  const { root, helper, data, run } = await fixture(); const shim = path.join(root, 'shellfox');
  const file = path.join(root, "it's a WSL script.sh"); await writeFile(file, 'printf "%s\\0" "$@"\n');
  const runtime = path.win32.join(path.dirname(helper), '../cli-runtime/node.exe');
  await writeFile(shim, shellfoxShims({ executable: process.execPath, cliHelper: helper, userData: data }).posix.replaceAll(runtime, process.execPath));
  const wsl = path.win32.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'wsl.exe');
  const prefix = ['--distribution', wslDistro!];
  const guestRoot = (await exec(wsl, [...prefix, '--exec', 'wslpath', '-u', root], { timeout: 10000 })).stdout.trim();
  const invoke = (args: string[]) => exec(wsl, [...prefix, '--cd', guestRoot, '--exec', '/bin/bash', '--', guestRoot + '/shellfox', ...args], { timeout: 20000 });
  await invoke(['cmd', '--register', 'wslargs', path.basename(file)]);
  const registry = JSON.parse(await readFile(path.join(data, 'cli-commands.json'), 'utf8'));
  expect(registry.commands[0]).toEqual({ name: 'wslargs', path: guestRoot + '/' + path.basename(file), distro: wslDistro });
  const result = await invoke(['cmd', 'wslargs', ...literalArgs]);
  expect(result.stdout.split('\0').slice(0, -1)).toEqual(literalArgs);
  const fromWindows = await run(['cmd', '--run', 'wslargs', ...literalArgs]);
  expect(fromWindows.code, fromWindows.stderr).toBe(0);
  expect(fromWindows.stdout.split('\0').slice(0, -1)).toEqual(literalArgs);
}, 30000);
it.skipIf(!linuxNode)('runs the native Linux CLI with path spaces, latest contents and literal arguments', async () => {
  const { root } = await fixture();
  const wsl = path.win32.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'wsl.exe');
  const prefix = ['--distribution', wslDistro!];
  const guestRoot = (await exec(wsl, [...prefix, '--exec', 'wslpath', '-u', root], { timeout: 10000 })).stdout.trim();
  const shim = path.join(root, 'linux-shellfox'), guestData = guestRoot + '/linux data';
  await writeFile(shim, linuxLauncher({ executable: linuxNode!, cliHelper: guestRoot + '/main/cli.cjs', userData: guestData }));
  const file = path.join(root, 'linux script.sh'); await writeFile(file, 'test -z "${ELECTRON_RUN_AS_NODE:-}" || exit 91\nprintf "%s\\0" "$@"\n');
  const invoke = (args: string[]) => exec(wsl, [...prefix, '--cd', guestRoot, '--exec', '/bin/sh', guestRoot + '/linux-shellfox', ...args], { timeout: 20000 });
  await invoke(['cmd', '--register', 'linuxargs', path.basename(file)]);
  const result = await invoke(['cmd', '--run', 'linuxargs', ...literalArgs]);
  expect(result.stdout.split('\0').slice(0, -1)).toEqual(literalArgs);
  await writeFile(file, 'printf "latest Linux"');
  expect((await invoke(['cmd', 'linuxargs'])).stdout).toBe('latest Linux');
  await expect(invoke(['update', '--nope'])).rejects.toMatchObject({ code: 2 });
}, 30000);
