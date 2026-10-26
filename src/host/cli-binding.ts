import path from 'node:path';
import { access, lstat, realpath, rename, unlink, writeFile } from 'node:fs/promises';
import { constants } from 'node:fs';
import { randomUUID } from 'node:crypto';
import { z } from 'zod';
import type { CliPort } from '../main/platform/shellfox-cli';
import { posixHelper, quoteSh } from '../main/platform/cli-helper';
import { customScriptContext } from '../main/platform/shellfox-cli-scripts';
import { preparePrivateDirectory } from '../main/control-endpoint';
import { success, failure } from '../shared/contracts';
import type { HostResourceOptions } from './paths';

const bindingSchema = z.object({ executable: z.string().min(1).refine(path.isAbsolute), entry: z.string().min(1).refine(path.isAbsolute), runAsNode: z.boolean() }).strict().refine(value => !/[\x00-\x1f]/.test(value.executable + value.entry));
export type HostCliBinding = z.infer<typeof bindingSchema>;
const ps = (value: string): string => "'" + value.replaceAll("'", "''") + "'";

/** Private shims default to this host's data path. Explicit --user-data remains available. */
export async function createBoundHostCli(userData: string, raw: HostCliBinding, options: HostResourceOptions & { runtimePath?: string } = {}): Promise<CliPort> {
  const binding = bindingSchema.parse(raw);
  await access(binding.executable, process.platform === 'win32' ? constants.F_OK : constants.X_OK);
  await access(binding.entry, constants.R_OK);
  if (!(await lstat(binding.entry)).isFile()) throw new Error('Host CLI binding requires a real CLI bundle file.');
  const canonical = await realpath(userData), host = path.join(canonical, 'host'), binDir = path.join(host, 'bin');
  await preparePrivateDirectory(canonical, host); await preparePrivateDirectory(host, binDir);
  const environment = { SHELLFOX_HEADLESS: '1', SHELLFOX_USER_DATA: canonical,
    ...(options.resourcesPath ? { SHELLFOX_RESOURCES_PATH: options.resourcesPath } : {}),
    ...(options.projectRoot ? { SHELLFOX_PROJECT_ROOT: options.projectRoot } : {}),
    ...(options.runtimePath ? { SHELLFOX_RUNTIME_PATH: options.runtimePath } : {}) };
  const powershell = `$ErrorActionPreference='Stop'
${binding.runAsNode ? "$env:ELECTRON_RUN_AS_NODE='1'" : 'Remove-Item Env:ELECTRON_RUN_AS_NODE -ErrorAction SilentlyContinue'}
Remove-Item Env:NODE_PATH,Env:NODE_OPTIONS,Env:SHELLFOX_RESOURCES_PATH,Env:SHELLFOX_PROJECT_ROOT,Env:SHELLFOX_RUNTIME_PATH -ErrorAction SilentlyContinue
${Object.entries(environment).map(([name, value]) => `$env:${name}=${ps(value)}`).join('\n')}
$json=ConvertTo-Json -InputObject (@('--shellfox-default-user-data',${ps(canonical)}) + @($args)) -Compress
$encoded=[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($json))
& ${ps(binding.executable)} ${ps(binding.entry)} --shellfox-argv-json $encoded
exit $LASTEXITCODE
`;
  const shell = '#!/bin/sh\nset -eu\nunset NODE_PATH NODE_OPTIONS\nexport SHELLFOX_HEADLESS=1\nexport SHELLFOX_USER_DATA=' + quoteSh(canonical) + '\n' + customScriptContext(posixHelper({ executable: binding.executable, runtime: binding.executable, cliHelper: binding.entry, userData: canonical, runAsNode: binding.runAsNode, resourcesPath: options.resourcesPath, projectRoot: options.projectRoot, runtimePath: options.runtimePath }, process.platform === 'win32'));
  const cmd = '@echo off\r\nsetlocal DisableDelayedExpansion\r\n"%SystemRoot%\\System32\\WindowsPowerShell\\v1.0\\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "%~dp0shellfox.ps1" %*\r\nexit /b %errorlevel%\r\n';
  for (const [name, content] of Object.entries({ shellfox: shell, 'shellfox.ps1': '\ufeff' + powershell, 'shellfox.cmd': cmd })) {
    const stage = path.join(binDir, randomUUID() + '.tmp');
    try { await writeFile(stage, content, { flag: 'wx', mode: 0o700 }); await rename(stage, path.join(binDir, name)); }
    finally { await unlink(stage).catch(error => { if (!(error instanceof Error && 'code' in error && error.code === 'ENOENT')) throw error; }); }
  }
  const state = { supported: false, installed: true, command: 'shellfox', reason: 'Private CLI shims default to this host. Global PATH changes are managed by the CLI installer.' };
  return { binDir, readOnly: true, get: async () => success(state), set: async () => failure('UNSUPPORTED', state.reason) };
}
