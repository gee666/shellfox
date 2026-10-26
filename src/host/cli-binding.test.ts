import path from 'node:path';
import { execFile, spawnSync } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { afterEach, expect, it } from 'vitest';
import { createBoundHostCli } from './cli-binding';
import { findGitBash, quoteMsysArgument, scriptContext } from '../main/platform/shellfox-cli-scripts';

const run = promisify(execFile), directories: string[] = [];
const posixShell = process.platform === 'win32' ? await findGitBash(scriptContext()).catch(() => null) : '/bin/sh';
const transports: Array<'native' | 'posix'> = process.platform === 'win32' ? ['native', 'posix'] : ['posix'];
afterEach(async () => { for (const directory of directories.splice(0)) await rm(directory, { recursive: true, force: true }); });
async function execute(binDir: string, args: string[], env: NodeJS.ProcessEnv, transport: 'native' | 'posix'): Promise<unknown> {
  if (transport === 'posix') {
    if (!posixShell) throw new Error('POSIX shell unavailable.');
    const script = path.join(binDir, 'shellfox').replaceAll('\\', '/');
    const argv = [script, ...args];
    const result = spawnSync(posixShell, process.platform === 'win32' ? argv.map(quoteMsysArgument) : argv, {
      env: { ...env, MSYS_ARG_CONV_EXCL: '*' }, timeout: 10_000, encoding: 'utf8',
      windowsVerbatimArguments: process.platform === 'win32',
      argv0: process.platform === 'win32' ? quoteMsysArgument(posixShell) : posixShell,
    });
    if (result.error) throw result.error;
    expect(result.status, result.stderr).toBe(0);
    return JSON.parse(result.stdout);
  }
  const value = Buffer.from(JSON.stringify({ script: path.join(binDir, 'shellfox.ps1'), args })).toString('base64');
  const script = `$p=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('${value}'))|ConvertFrom-Json; $a=@($p.args); & $p.script @a`;
  return JSON.parse((await run(path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/WindowsPowerShell/v1.0/powershell.exe'), ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', Buffer.from(script, 'utf16le').toString('base64')], { env, timeout: 10_000 })).stdout);
}
for (const transport of transports) {
it.skipIf(transport === 'posix' && !posixShell)(`binds literal arguments and custom data independently through ${transport} without leaking foreign runtime paths`, async () => {
  const executeBound = (binDir: string, args: string[], env: NodeJS.ProcessEnv) => execute(binDir, args, env, transport);
  await mkdir('tmp', { recursive: true }); const root = await mkdtemp(path.resolve("tmp/host-bound-Ю ' &%-")); directories.push(root);
  const entry = path.join(root, 'cli.cjs');
  await writeFile(entry, `let args=process.argv.slice(2); if(args[0]==='--shellfox-argv-json')args=JSON.parse(Buffer.from(args[1],'base64').toString('utf8')); console.log(JSON.stringify({args,electron:process.env.ELECTRON_RUN_AS_NODE,resources:process.env.SHELLFOX_RESOURCES_PATH,project:process.env.SHELLFOX_PROJECT_ROOT,runtime:process.env.SHELLFOX_RUNTIME_PATH}));`);
  const a = path.join(root, 'data-a'), b = path.join(root, 'data-b'); await mkdir(a); await mkdir(b);
  const resourcesPath = path.join(root, 'own resources'), projectRoot = path.join(root, 'own project'), runtimePath = path.join(root, 'verified runtime');
  const cliA = await createBoundHostCli(a, { executable: process.execPath, entry, runAsNode: false }, { resourcesPath, projectRoot, runtimePath });
  const cliB = await createBoundHostCli(b, { executable: process.execPath, entry, runAsNode: true }, { resourcesPath, projectRoot });
  const env = { ...process.env, ELECTRON_RUN_AS_NODE: '1', SHELLFOX_RUNTIME_PATH: 'foreign runtime', SHELLFOX_PROJECT_ROOT: 'foreign project', SHELLFOX_RESOURCES_PATH: 'foreign resources' };
  const args = ['api', 'literal"quote', 'two words', "apostrophe's", '&%', '--json'];
  const expectedA = { args: ['--shellfox-default-user-data', a, ...args], resources: resourcesPath, project: projectRoot, runtime: runtimePath };
  expect(await executeBound(cliA.binDir, args, env)).toEqual(expectedA);
  expect(await executeBound(cliB.binDir, args, env)).toEqual({ args: ['--shellfox-default-user-data', b, ...args], electron: '1', resources: resourcesPath, project: projectRoot });
  expect(await executeBound(cliA.binDir, args, env)).toEqual(expectedA);
  expect(await executeBound(cliA.binDir, ['--user-data', b, ...args], env)).toEqual({ ...expectedA, args: ['--shellfox-default-user-data', a, '--user-data', b, ...args] });
}, 20_000);
}
