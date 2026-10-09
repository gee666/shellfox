import { execFile, spawn } from 'node:child_process';
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { promisify } from 'node:util';
import { build } from 'esbuild';
import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';
import { powershellCommitGate, startHandoff, type InstallHandoff } from './handoff';
import { createUpdatePlatform } from './platform-installer';

const exec = promisify(execFile);
// Deliberately use Windows PowerShell 5.1, not pwsh or a PATH shim.
const powershell = path.win32.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/WindowsPowerShell/v1.0/powershell.exe');
const psArgs = () => ['-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass'];

async function scratch(): Promise<string> {
  await mkdir(path.resolve('tmp'), { recursive: true });
  return mkdtemp(path.resolve("tmp/handoff windows' ;&()-"));
}

async function exists(file: string): Promise<boolean> {
  try { await readFile(file); return true; }
  catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT') return false; throw error; }
}

async function waitUntil(check: () => Promise<boolean>, description: string, timeout = 10_000): Promise<void> {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (await check()) return;
    await delay(25);
  }
  throw new Error(`Timed out waiting for ${description}.`);
}

function alive(pid: number): boolean {
  try { process.kill(pid, 0); return true; }
  catch (error) { if ((error as NodeJS.ErrnoException).code === 'ESRCH') return false; throw error; }
}

async function killTree(pid: number | undefined): Promise<void> {
  if (pid && alive(pid)) {
    await exec('taskkill.exe', ['/PID', String(pid), '/T', '/F'], { timeout: 5000, windowsHide: true }).catch(() => {});
    await waitUntil(async () => !alive(pid), `process ${pid} to stop`, 5000);
  }
}

function runParent(parent: string, args: string[], root: string) {
  // Shellfox is a GUI app with no console. Give the fixture parent no console
  // too, rather than testing console teardown when a console-owning Node exits.
  const child = spawn(process.execPath, [parent, ...args], {
    detached: true, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true, timeout: 25_000,
    env: { ...process.env, TEMP: root, TMP: root },
  });
  let stdout = '', stderr = '';
  child.stdout.setEncoding('utf8').on('data', (text: string) => { stdout = (stdout + text).slice(-64 * 1024); });
  child.stderr.setEncoding('utf8').on('data', (text: string) => { stderr = (stderr + text).slice(-64 * 1024); });
  const result = new Promise<{ stdout: string; stderr: string }>((resolve, reject) => {
    child.once('error', reject);
    child.once('close', (code, signal) => {
      if (code === 0) resolve({ stdout, stderr });
      else reject(new Error(`Fixture parent exited with ${code ?? signal}: ${stderr}`));
    });
  });
  return { child, result };
}

const benignHelper = `param([string]$Control, [int]$ParentPid)
$ErrorActionPreference = 'Stop'
$c = [PSCustomObject]@{ control = $Control; pid = $ParentPid }
[System.IO.File]::WriteAllText((Join-Path $Control 'helper.pid'), [string]$PID)
[System.IO.File]::WriteAllText((Join-Path $Control 'powershell.version'), $PSVersionTable.PSVersion.ToString())
try {
${powershellCommitGate}
$exitDeadline = [DateTime]::UtcNow.AddSeconds(10)
while (Get-Process -Id $ParentPid -ErrorAction SilentlyContinue) {
  if ([DateTime]::UtcNow -gt $exitDeadline) { throw 'Parent is still alive.' }
  Start-Sleep -Milliseconds 25
}
Start-Sleep -Milliseconds 300
[System.IO.File]::WriteAllText((Join-Path $Control 'after-parent-exit'), [string]$ParentPid)
} catch {
  [System.IO.File]::WriteAllText((Join-Path $Control 'helper.error'), ($_ | Out-String))
  exit 1
}
`;

// Bundle the actual implementation, so this test exercises spawn options and
// child.unref(), not a reimplementation or Vitest's mocked module graph.
const fixtureParent = `
import { startHandoff } from './src/main/update/handoff.ts';
const [powershell, helper, control, mode] = process.argv.slice(2);
const lease = await startHandoff(powershell, [
  '-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
  '-File', helper, '-Control', control, '-ParentPid', String(process.pid),
], control, 15000);
if (mode === 'abandon') {
  console.log('ready without commit');
  process.exit(0);
}
await lease.commit();
if (!lease.committed) throw new Error('Commit was not acknowledged.');
console.log('committed');
// No process.exit(): an incorrectly referenced helper would keep Node alive.
`;

describe.skipIf(process.platform !== 'win32')('Windows updater real process handoff', () => {
  let environmentRoot: string;
  beforeAll(async () => {
    environmentRoot = await scratch();
    vi.stubEnv('TEMP', environmentRoot);
    vi.stubEnv('TMP', environmentRoot);
    vi.stubEnv('PSModuleAnalysisCachePath', path.join(environmentRoot, 'ModuleAnalysisCache'));
    // A runner launched from pwsh can put incompatible PS7 modules first.
    // Use PS5.1's own modules, especially Get-FileHash's Utility module.
    vi.stubEnv('PSModulePath', path.win32.join(path.win32.dirname(powershell), 'Modules'));
  });
  afterAll(async () => {
    vi.unstubAllEnvs();
    if (environmentRoot) await rm(environmentRoot, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
  });

  it('runs the generated installer helper to readiness and cancels before commit', async () => {
    const root = await scratch();
    let lease: InstallHandoff | undefined;
    try {
      const executable = path.join(root, 'installation', 'app-0.1.0', 'Shellfox.exe');
      const updateExe = path.join(root, 'installation', 'Update.exe');
      const work = path.join(root, 'work');
      await mkdir(path.dirname(executable), { recursive: true });
      await mkdir(work);
      // These are inert text files, never copies of a real installer/updater.
      const inert = 'Not an executable. This regression test must never install anything.\n';
      await writeFile(updateExe, inert);
      const setup = path.join(work, 'ShellfoxSetup.exe');
      await writeFile(setup, inert);
      const platform = await createUpdatePlatform({ platform: 'win32', arch: 'x64', executable, packaged: true });
      expect(platform.supported, platform.reason ?? undefined).toBe(true);
      const launch = await platform.prepare(setup, '0.2.0', work);
      // detached:true can produce exit status 0 without executing install.ps1.
      // Only a real ready file from that script counts as successful startup.
      try { lease = await launch(); }
      catch (error) {
        const log = path.join(work, 'install.log');
        if (await exists(log)) {
          const bytes = await readFile(log);
          const message = bytes.subarray(0, 2).equals(Buffer.from([0xff, 0xfe]))
            ? bytes.subarray(2).toString('utf16le') : bytes.toString('utf8');
          throw new Error(message, { cause: error });
        }
        throw error;
      }
      const controls = (await readdir(work)).filter(name => name.startsWith('handoff-'));
      expect(controls).toHaveLength(1);
      const control = path.join(work, controls[0]!);
      expect(await exists(path.join(control, 'ready'))).toBe(true);
      expect(lease.committed).toBe(false);
      await delay(400); // Give the helper multiple commit-gate polling cycles.
      expect(await exists(path.join(control, 'commit'))).toBe(false);
      expect(await exists(path.join(control, 'committed'))).toBe(false);
      expect(await exists(path.join(work, 'install.log'))).toBe(false);
      const helperProcess = JSON.parse(await readFile(path.join(control, 'helper-process.json'), 'utf8')) as { pid: number };
      expect(alive(helperProcess.pid)).toBe(true);
      await lease.cancel(); // Resolves only after the real helper has exited.
      expect(alive(helperProcess.pid)).toBe(false);
      expect(await exists(path.join(control, 'helper-exited'))).toBe(true);
      expect(await exists(path.join(control, 'cancel'))).toBe(true);
      await expect(lease.commit()).rejects.toThrow('no longer ready');
      expect(lease.committed).toBe(false);
      expect(await exists(path.join(control, 'committed'))).toBe(false);
      expect(await readFile(setup, 'utf8')).toBe(inert);
      expect(await readFile(updateExe, 'utf8')).toBe(inert);
    } finally {
      try { await lease?.cancel(); }
      finally { await rm(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 }); }
    }
  }, 75_000); // createUpdatePlatform uses the production 60s readiness deadline.

  async function stubbornHelper(root: string, ready = true): Promise<string> {
    const helper = path.join(root, 'stubborn.ps1');
    await writeFile(helper, `param([string]$Control, [string]$Value)
[IO.File]::WriteAllText((Join-Path $Control 'argument'), $Value)
${ready ? "[IO.File]::WriteAllText((Join-Path $Control 'ready'), '')" : ''}
Start-Sleep -Seconds 60
`);
    return helper;
  }

  it('passes metacharacters and Windows argv escaping as data, then cancels a racing commit', async () => {
    const root = await scratch();
    let lease: InstallHandoff | undefined;
    try {
      const helper = await stubbornHelper(root);
      const value = 'quotes " backslash\\" ; & $env:TEMP ` %PATH% (test) \u00e9\u96ea trailing\\';
      lease = await startHandoff(powershell, [...psArgs(), '-File', helper, '-Control', root, '-Value', value], root, 5000);
      expect(await readFile(path.join(root, 'argument'), 'utf8')).toBe(value);
      const pending = expect(lease.commit()).rejects.toThrow();
      await lease.cancel();
      await pending;
      expect(lease.committed).toBe(false);
      expect(await exists(path.join(root, 'detach'))).toBe(false);
      const { pid } = JSON.parse(await readFile(path.join(root, 'helper-process.json'), 'utf8')) as { pid: number };
      expect(alive(pid)).toBe(false);
    } finally {
      try { await lease?.cancel(); }
      finally { await rm(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 }); }
    }
  }, 15_000);

  it.each(['ready', 'committed'] as const)('kills the actual independent helper after a %s acknowledgment timeout', async phase => {
    const root = await scratch();
    let helperPid: number | undefined;
    try {
      const helper = await stubbornHelper(root, phase !== 'ready');
      const launch = () => startHandoff(powershell, [...psArgs(), '-File', helper, '-Control', root], root, 3000);
      if (phase === 'ready') {
        await expect(launch()).rejects.toThrow('acknowledge');
      } else {
        const lease = await launch();
        try { await expect(lease.commit()).rejects.toThrow('acknowledge'); }
        finally { await lease.cancel(); }
        expect(lease.committed).toBe(false);
      }
      expect(await exists(path.join(root, 'detach'))).toBe(false);
      helperPid = (JSON.parse(await readFile(path.join(root, 'helper-process.json'), 'utf8')) as { pid: number }).pid;
      expect(alive(helperPid)).toBe(false);
      expect(await exists(path.join(root, 'helper-exited'))).toBe(true);
    } finally {
      if (!helperPid) {
        try { helperPid = (JSON.parse(await readFile(path.join(root, 'helper-process.json'), 'utf8')) as { pid: number }).pid; }
        catch { /* Bootstrap did not launch. */ }
      }
      try { await killTree(helperPid); }
      finally { await rm(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 }); }
    }
  }, 15_000);

  it('does not certify cancellation merely because the bootstrap died', async () => {
    const root = await scratch();
    let helperPid: number | undefined;
    let bootstrapPid: number | undefined;
    try {
      const helper = await stubbornHelper(root);
      const lease = await startHandoff(powershell, [...psArgs(), '-File', helper, '-Control', root], root, 5000);
      const info = JSON.parse(await readFile(path.join(root, 'helper-process.json'), 'utf8')) as { pid: number; bootstrapPid: number };
      helperPid = info.pid; bootstrapPid = info.bootstrapPid;
      // Kill only the bootstrap, not its independent process tree.
      process.kill(bootstrapPid);
      await waitUntil(async () => !alive(bootstrapPid!), 'bootstrap exit');
      expect(alive(helperPid)).toBe(true);
      await expect(lease.cancel()).rejects.toThrow('cancellation was not confirmed');
      expect(lease.committed).toBe(false);
      expect(alive(helperPid)).toBe(true);
    } finally {
      try { await killTree(bootstrapPid); }
      finally {
        try { await killTree(helperPid); }
        finally { await rm(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 }); }
      }
    }
  }, 20_000);

  for (const mode of ['commit', 'abandon'] as const) {
    it(mode === 'commit'
      ? 'lets an acknowledged helper outlive its naturally exiting parent'
      : 'exits without doing work when its parent dies before commit', async () => {
      const root = await scratch();
      let parentPid: number | undefined;
      let helperPid: number | undefined;
      try {
        const helper = path.join(root, 'benign.ps1');
        const parent = path.join(root, 'parent.mjs');
        await writeFile(helper, benignHelper);
        await build({
          stdin: { contents: fixtureParent, resolveDir: process.cwd(), sourcefile: 'handoff-fixture-parent.ts', loader: 'ts' },
          bundle: true, platform: 'node', target: 'node22', format: 'esm', outfile: parent,
        });
        const parentRun = runParent(parent, [powershell, helper, root, mode], root);
        parentPid = parentRun.child.pid;
        const outcome = await parentRun.result;
        helperPid = Number(await readFile(path.join(root, 'helper.pid'), 'utf8'));
        expect(Number.isSafeInteger(helperPid) && helperPid > 0).toBe(true);
        expect(outcome.stderr).toBe('');
        expect(outcome.stdout.trim()).toBe(mode === 'commit' ? 'committed' : 'ready without commit');
        expect(parentPid).toBeDefined();
        expect(alive(parentPid!)).toBe(false);
        expect(await readFile(path.join(root, 'powershell.version'), 'utf8')).toMatch(/^5\.1\./);
        expect(await exists(path.join(root, 'ready'))).toBe(true);
        if (mode === 'commit') {
          expect(await exists(path.join(root, 'commit'))).toBe(true);
          expect(await exists(path.join(root, 'committed'))).toBe(true);
          await waitUntil(async () => {
            if (await exists(path.join(root, 'after-parent-exit'))) return true;
            if (!alive(helperPid!)) {
              const errorFile = path.join(root, 'helper.error');
              throw new Error(await exists(errorFile)
                ? await readFile(errorFile, 'utf8') : 'Helper exited after commit without writing its parent-exit marker.');
            }
            return false;
          }, 'work after committed parent exit');
          expect(await readFile(path.join(root, 'after-parent-exit'), 'utf8')).toBe(String(parentPid));
        } else {
          expect(await exists(path.join(root, 'commit'))).toBe(false);
          expect(await exists(path.join(root, 'committed'))).toBe(false);
        }
        await waitUntil(async () => !alive(helperPid!), 'helper exit');
        if (mode === 'abandon') expect(await exists(path.join(root, 'after-parent-exit'))).toBe(false);
      } finally {
        // A failed startup assertion or parent timeout must not orphan PowerShell.
        try {
          await killTree(parentPid);
        } finally {
          try {
            if (!helperPid) {
              try { helperPid = Number(await readFile(path.join(root, 'helper.pid'), 'utf8')); }
              catch { /* The script never started. */ }
            }
            await killTree(helperPid);
          } finally {
            await rm(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
          }
        }
      }
    }, 50_000);
  }
});
