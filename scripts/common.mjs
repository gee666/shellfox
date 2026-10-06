import { mkdirSync } from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
export const root = process.cwd();
export const scratch = path.join(root, 'tmp');
mkdirSync(scratch, { recursive: true });
export const env = { ...process.env, TMP: scratch, TEMP: scratch, TMPDIR: scratch, npm_config_cache: path.join(scratch, 'npm-cache'), ELECTRON_CACHE: path.join(scratch, 'electron-cache'), ELECTRON_BUILDER_CACHE: path.join(scratch, 'build-cache'), DOTNET_CLI_HOME: path.join(scratch, 'dotnet-home'), DOTNET_CLI_TELEMETRY_OPTOUT: '1', DOTNET_SKIP_FIRST_TIME_EXPERIENCE: '1', DOTNET_GENERATE_ASPNET_CERTIFICATE: 'false', NUGET_PACKAGES: path.join(scratch, 'nuget'), PLAYWRIGHT_BROWSERS_PATH: path.join(scratch, 'playwright-browsers') };
export function run(executable, args, timeout = 300000, extraEnv = {}) {
  const result = spawnSync(executable, args, { env: { ...env, ...extraEnv }, stdio: 'inherit', shell: false, timeout, windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(executable + ' exited ' + result.status);
}
export const runNode = (script, args = [], timeout = 300000, extraEnv = {}) => run(process.execPath, [script, ...args], timeout, extraEnv);
