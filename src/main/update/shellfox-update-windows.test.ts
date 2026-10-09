import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import path from 'node:path';
import { execFile, type ChildProcess, type ExecFileException } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import { shellfoxShims } from '../platform/shellfox-cli';
import updatePs1 from './shellfox-update.ps1';

const exec = promisify(execFile);
const quote = (value: string) => "'" + value.replaceAll("'", "''") + "'";
const powershell = (code: string, env: NodeJS.ProcessEnv) => exec('powershell.exe', [
  '-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(code, 'utf16le').toString('base64'),
], { env, timeout: 20000 });
let root = '', installer = '';

// A Squirrel-like installer exits after launching an app that remains alive.
// Use a GUI executable so its child cannot hold the test runner's stdio open.
const fixture = `using System;
using System.Diagnostics;
using System.IO;
using System.Threading;
public class Installer {
  public static void Main(string[] args) {
    if (args.Length > 0 && args[0] == "child") { Thread.Sleep(60000); return; }
    var child = Process.Start(new ProcessStartInfo(Environment.GetEnvironmentVariable("TEST_APP"), "child") { UseShellExecute = true });
    File.WriteAllText(Environment.GetEnvironmentVariable("TEST_STARTED"), child.Id.ToString());
    var gate = Environment.GetEnvironmentVariable("TEST_FINISH");
    var deadline = DateTime.UtcNow.AddSeconds(30);
    while (!File.Exists(gate) && DateTime.UtcNow < deadline) Thread.Sleep(25);
    Environment.Exit(Int32.Parse(Environment.GetEnvironmentVariable("TEST_EXIT")));
  }
}`;

async function eventually<T>(read: () => Promise<T | undefined>, timeout = 8000): Promise<T> {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    const value = await read();
    if (value !== undefined) return value;
    await new Promise(resolve => setTimeout(resolve, 25));
  }
  throw new Error('CLI updater did not finish within the regression-test deadline.');
}
const killTree = (pid: number) => exec('taskkill.exe', ['/PID', String(pid), '/T', '/F'], { timeout: 5000 }).catch(() => {});

describe.skipIf(process.platform !== 'win32')('Windows CLI updater process lifetime', () => {
  beforeAll(async () => {
    await mkdir(path.resolve('tmp'), { recursive: true });
    root = await mkdtemp(path.resolve('tmp/cli updater-'));
    installer = path.join(root, 'ShellfoxSetup.exe');
    // Keep compiler scratch files inside the project's tmp directory too.
    await powershell(`$ErrorActionPreference='Stop'; Add-Type -TypeDefinition ${quote(fixture)} -OutputAssembly ${quote(installer)} -OutputType WindowsApplication`, { ...process.env, TEMP: root, TMP: root });
  }, 30000);
  afterAll(async () => { if (root) await rm(root, { recursive: true, force: true }); });

  async function run(route: 'powershell' | 'cmd', installerExit = 0, check = false) {
    const scratch = await mkdtemp(path.join(root, 'run-'));
    const app = path.join(scratch, 'install', 'app-0.1.1', 'Shellfox.exe');
    const bin = path.join(scratch, 'bin'), tmp = path.join(scratch, 'tmp');
    await mkdir(path.dirname(app), { recursive: true }); await mkdir(bin); await mkdir(tmp);
    await writeFile(app, 'fixture');
    await writeFile(path.join(path.dirname(path.dirname(app)), 'Update.exe'), 'fixture');
    const api = path.join(scratch, 'release.json');
    await writeFile(api, JSON.stringify({ tag_name: 'v0.2.0', assets: [{ name: 'ShellfoxSetup.exe', browser_download_url: pathToFileURL(installer).href }] }));
    const shims = shellfoxShims({ executable: app, updateScript: updatePs1 });
    await writeFile(path.join(bin, 'shellfox.cmd'), shims.cmd);
    const script = path.join(bin, 'shellfox-update.ps1'); await writeFile(script, shims.update);
    const started = path.join(scratch, 'started'), finish = path.join(scratch, 'finish');
    const env = { ...process.env, TEMP: tmp, TMP: tmp, SHELLFOX_UPDATE_API: pathToFileURL(api).href,
      TEST_APP: installer, TEST_STARTED: started, TEST_FINISH: finish, TEST_EXIT: String(installerExit) };
    let result: { code: number | string; out: string; err: string } | undefined;
    let childPid: number | undefined;
    let updater: ChildProcess | undefined;
    try {
      const done = (error: ExecFileException | null, out: string, err: string) => { result = { code: error?.code ?? 0, out, err }; };
      updater = route === 'cmd'
        ? execFile('cmd.exe', ['/d', '/c', 'shellfox.cmd', 'update', ...(check ? ['--check'] : [])], { cwd: bin, env }, done)
        : execFile('powershell.exe', ['-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script, '-Executable', app, ...(check ? ['-Check'] : [])], { env }, done);
      if (!check) {
        childPid = await eventually(async () => {
          try { return Number(await readFile(started, 'utf8')); } catch { return undefined; }
        });
        // The real installer has not exited yet. Do not report success early.
        await new Promise(resolve => setTimeout(resolve, 200));
        expect(result).toBeUndefined();
        await writeFile(finish, 'finish');
      }
      const completed = await eventually(async () => result);
      if (childPid) expect(() => process.kill(childPid!, 0)).not.toThrow();
      expect(await readdir(tmp)).toEqual([]);
      if (check) await expect(readFile(started)).rejects.toMatchObject({ code: 'ENOENT' });
      return completed;
    } finally {
      // Also clean up the old -Wait implementation's blocked process tree on failure.
      if (!result && updater?.pid) await killTree(updater.pid);
      if (!childPid) { try { childPid = Number(await readFile(started, 'utf8')); } catch { /* Installer never ran. */ } }
      if (childPid) await killTree(childPid);
    }
  }

  for (const route of ['powershell', 'cmd'] as const) {
    it(`returns success through ${route} while the installed app remains running`, async () => {
      const result = await run(route);
      expect(result.code, result.err).toBe(0);
      expect(result.out).toContain('Shellfox 0.2.0 installed.');
      expect(result.out).toContain('Restart Shellfox to use 0.2.0');
    }, 25000);
  }
  it('returns failure through the CMD shim and cleans up without waiting for the app', async () => {
    const result = await run('cmd', 37);
    expect(result.code).toBe(1);
    expect(result.err).toContain('the installer failed (exit code 37)');
    expect(result.out).not.toContain('Shellfox 0.2.0 installed.');
  }, 25000);
  it('--check exits without starting an installer', async () => {
    const result = await run('cmd', 0, true);
    expect(result.code, result.err).toBe(0);
    expect(result.out).toContain('Shellfox 0.2.0 is available');
  }, 15000);
});
