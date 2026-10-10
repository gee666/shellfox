import { mkdirSync, writeFileSync } from 'node:fs';
import { release as osRelease } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
export const root = process.cwd();
export const scratch = path.join(root, 'tmp');
mkdirSync(scratch, { recursive: true });
export const env = { ...process.env, TMP: scratch, TEMP: scratch, TMPDIR: scratch, npm_config_cache: path.join(scratch, 'npm-cache'), ELECTRON_CACHE: path.join(scratch, 'electron-cache'), ELECTRON_BUILDER_CACHE: path.join(scratch, 'build-cache'), DOTNET_CLI_HOME: path.join(scratch, 'dotnet-home'), DOTNET_CLI_TELEMETRY_OPTOUT: '1', DOTNET_SKIP_FIRST_TIME_EXPERIENCE: '1', DOTNET_GENERATE_ASPNET_CERTIFICATE: 'false', NUGET_PACKAGES: path.join(scratch, 'nuget'), PLAYWRIGHT_BROWSERS_PATH: path.join(scratch, 'playwright-browsers') };
export function run(executable, args, timeout = 300000, extraEnv = {}) {
  const command = [executable, ...args].map(value => JSON.stringify(value)).join(' ');
  console.log('[run] ' + command);
  const childEnv = { ...env, ...extraEnv };
  const result = spawnSync(executable, args, { env: childEnv, stdio: 'inherit', shell: false, timeout, windowsHide: true });
  if (result.error || result.status !== 0) {
    const hex = typeof result.status === 'number' ? '0x' + (result.status >>> 0).toString(16).toUpperCase().padStart(8, '0') : null;
    const details = { command, executable, args, cwd: root, status: result.status, hex, signal: result.signal, error: result.error?.message, node: process.version, platform: process.platform, arch: process.arch, os: osRelease(), paths: Object.fromEntries(['TMP','TEMP','TMPDIR','ELECTRON_CACHE','npm_config_cache'].map(key => [key,childEnv[key]])) };
    const directory = path.join(scratch,'build-diagnostics'); mkdirSync(directory,{recursive:true});
    const file = path.join(directory,'child-failure-' + Date.now() + '-' + process.pid + '.json');
    writeFileSync(file, JSON.stringify(details,null,2));
    const failFast = hex === '0xC0000409' ? ' (Windows native fail-fast; no JavaScript exception)' : '';
    throw new Error(`Child command failed: ${command}; status=${result.status}${hex?' '+hex:''}${failFast}; signal=${result.signal ?? 'none'}; cwd=${root}; diagnostics=${file}${result.error?'\n'+result.error.message:''}`, { cause: result.error });
  }
}
export const runNode = (script, args = [], timeout = 300000, extraEnv = {}) => run(process.execPath, [script, ...args], timeout, extraEnv);
