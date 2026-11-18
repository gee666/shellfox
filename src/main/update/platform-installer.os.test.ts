import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { it } from 'vitest';
import { assertWindowsHandoffs } from './platform-installer-test-fixture';

// HOLD. Parse with Windows PowerShell 5.1 without executing any installer code.
it.skipIf(process.platform !== 'win32')('uses fresh handoff controls on every Windows attempt and only installs after commit and PID exit', async () => {
  await assertWindowsHandoffs((file, script, config, asset) =>
    promisify(execFile)(file, ['-NoProfile', '-NonInteractive', '-Command', "$tokens=$null;$errors=$null;[System.Management.Automation.Language.Parser]::ParseFile($env:SHELLFOX_PARSE_SCRIPT,[ref]$tokens,[ref]$errors)|Out-Null;if($errors.Count){$errors|Out-String|Write-Error;exit 1};$c=Get-Content -LiteralPath $env:SHELLFOX_PARSE_CONFIG -Raw -Encoding UTF8|ConvertFrom-Json;if($c.file -cne $env:SHELLFOX_PARSE_ASSET){exit 2}"], { timeout: 10_000, windowsHide: true, env: { ...process.env, SHELLFOX_PARSE_SCRIPT: script, SHELLFOX_PARSE_CONFIG: config, SHELLFOX_PARSE_ASSET: asset } }));
});
