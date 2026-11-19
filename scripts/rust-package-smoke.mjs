import assert from 'node:assert/strict';
import { helperProtocolVersion } from './tailscale.mjs';
import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import { assertHeadlessFiles, executableName, fileHash, filesUnder, readJson, run, scratchRoot } from './rust-package-common.mjs';

export function verifyManifest(directory) {
  const manifest = readJson(path.join(directory, 'package-manifest.json'));
  assert.equal(manifest.format, 'shellfox-rust-portable-v1');
  assert.ok(['desktop', 'headless'].includes(manifest.kind));
  assert.equal(manifest.platform, process.platform, 'Smoke must run on the native package OS');
  assert.equal(manifest.arch, process.arch, 'Smoke must run on the native package architecture');
  assert.equal(manifest.update.supported, false);
  const paths = new Set();
  for (const file of manifest.files) {
    assert.equal(typeof file.path, 'string');
    assert.ok(!file.path.includes('\\') && !file.path.startsWith('/') && !file.path.includes(':'));
    assert.ok(file.path.split('/').every(part => part && part !== '.' && part !== '..'), 'Unsafe manifest path');
    assert.ok(!paths.has(file.path), 'Duplicate manifest file');
    paths.add(file.path);
    assert.match(file.sha256, /^[a-f0-9]{64}$/);
    assert.equal(fileHash(path.join(directory, file.path)), file.sha256, `Changed package file: ${file.path}`);
  }
  assert.deepEqual(filesUnder(directory).filter(file => file !== 'package-manifest.json').sort(), [...paths].sort(), 'Unmanifested package files');
  if (manifest.kind === 'headless') assertHeadlessFiles([...paths]);
  return manifest;
}

export async function ownHostSmoke(cli, helper, work, helperSha256, env = {}) {
  const data = path.join(work, 'isolated host data');
  mkdirSync(data, { mode: 0o700 });
  const flags = ['--data-dir', data, '--helper', helper, '--helper-sha256', helperSha256];
  const help = run(cli, ['--help'], { timeout: 15_000 });
  assert.match(help, /host stop/, 'Safe package smoke requires the actual CLI graceful host stop command');
  const child = spawn(cli, [...flags, 'host', 'serve'], {
    cwd: work, env: { ...process.env, ...env, TMP: work, TEMP: work, TMPDIR: work },
    windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'],
  });
  let diagnostics = '';
  child.stdout.on('data', bytes => { diagnostics = (diagnostics + bytes.toString()).slice(-16000); });
  child.stderr.on('data', bytes => { diagnostics = (diagnostics + bytes.toString()).slice(-16000); });
  const exited = new Promise((resolve, reject) => { child.once('exit', (code, signal) => resolve({ code, signal })); child.once('error', reject); });
  // Handle spawn failure even if the readiness loop has not reached await exited.
  exited.catch(() => {});
  const invoke = args => run(cli, [...flags, ...args], { cwd: work, env, timeout: 15_000 });
  const command = args => JSON.parse(invoke(args));
  let stopped = false;
  try {
    const deadline = Date.now() + 30_000;
    let ready = false;
    while (Date.now() < deadline && child.exitCode === null && child.signalCode === null) {
      try { assert.deepEqual(command(['session', 'list']), []); ready = true; break; }
      catch { await delay(100); }
    }
    assert.ok(ready, `Owned host did not become ready: ${diagnostics}`);
    command(['session', 'create', '--cwd', work, '--profile', process.platform === 'win32' ? 'cmd' : 'login-shell', '--title', 'Packaged metadata smoke']);
    const sessions = command(['session', 'list']);
    assert.equal(sessions.length, 1);
    const id = sessions[0].id;
    command(['session', 'rename', id, 'Renamed packaged smoke']);
    assert.equal(command(['session', 'list'])[0].title, 'Renamed packaged smoke');
    assert.ok(command(['settings', 'get']));
    command(['session', 'close', id]);
    assert.deepEqual(command(['session', 'list']), []);
    invoke(['host', 'stop']);
    const result = await Promise.race([exited, delay(15_000).then(() => { throw new Error('Owned host did not exit after stop'); })]);
    assert.deepEqual(result, { code: 0, signal: null });
    stopped = true;
    return 'owned persistent host: native endpoint, metadata create/list/rename/close, settings, graceful stop';
  } finally {
    if (!stopped && child.exitCode === null && child.signalCode === null) {
      try { invoke(['host', 'stop']); } catch { /* Only this smoke's private endpoint was contacted. */ }
      const ended = await Promise.race([exited.then(() => true).catch(() => true), delay(5_000).then(() => false)]);
      if (!ended) {
        // Never force-stop a host, even our fixture. Preserve its isolated data.
        const error = new Error(`Owned smoke host PID ${child.pid} did not stop. Scratch retained: ${work}`);
        error.preserveScratch = true;
        child.stdout.destroy();
        child.stderr.destroy();
        child.unref();
        throw error;
      }
    }
  }
}

export async function smokePackage(directory) {
  directory = path.resolve(directory);
  const manifest = verifyManifest(directory);
  mkdirSync(scratchRoot, { recursive: true });
  const work = mkdtempSync(path.join(scratchRoot, 'rust-package smoke '));
  const checks = ['archive file inventory and SHA-256 verification'];
  let preserveScratch = false;
  try {
    const helper = path.join(directory, 'resources', 'tailscale', executableName('shellfox-tailscale'));
    const probe = JSON.parse(run(helper, ['--probe'], { cwd: work, timeout: 15_000 }));
    assert.equal(probe.protocolVersion, helperProtocolVersion);
    assert.equal(probe.tailscaleVersion, '1.104.2');
    checks.push('offline embedded Tailscale probe, no enrollment');
    const cli = path.join(directory, 'bin', executableName('shellfox-cli'));
    const launcher = path.join(directory, process.platform === 'win32' ? 'shellfox.ps1' : 'shellfox');
    const help = process.platform === 'win32'
      ? run(path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/WindowsPowerShell/v1.0/powershell.exe'), ['-NoProfile', '-NonInteractive', '-File', launcher, '--help'], { cwd: work, timeout: 15_000 })
      : run(launcher, ['--help'], { cwd: work, timeout: 15_000 });
    assert.match(help, /--data-dir/);
    assert.ok(existsSync(cli));
    checks.push('CLI launcher from a path with spaces');
    const helperPath = `resources/tailscale/${executableName('shellfox-tailscale')}`;
    const helperDigest = manifest.files.find(file => file.path === helperPath)?.sha256;
    assert.match(helperDigest ?? '', /^[a-f0-9]{64}$/);
    checks.push(await ownHostSmoke(cli, helper, work, helperDigest));
    if (manifest.kind === 'desktop') {
      const desktop = path.join(directory, 'bin', executableName('shellfox'));
      const output = run(desktop, ['--data-dir', path.join(work, 'desktop data'), '--smoke'], { cwd: work, timeout: 60_000 });
      assert.match(output, /PASS: owned PTY/);
      checks.push('desktop --smoke: owned PTY, Ghostty output, input, resize and cleanup; no GUI window');
      if (process.platform === 'win32') {
        const elevated = path.join(directory, 'resources/elevated-terminal/Shellfox.ElevatedTerminal.exe');
        assert.equal(run(elevated, ['probe'], { timeout: 15_000 }).trim(), 'shellfox-elevated-conpty-v1');
        checks.push('retained self-contained elevated helper probe, no UAC');
      }
    }
    return { format: 'shellfox-rust-package-acceptance-v1', kind: manifest.kind, target: manifest.target,
      checks, fullPlatformAcceptance: false,
      notTested: ['GUI feature parity', 'administrator UAC launch', 'service registration/reboot', 'remote enrollment', 'updates', 'signing/notarization', 'mobile'] };
  } catch (error) {
    preserveScratch = error.preserveScratch === true;
    throw error;
  } finally { if (!preserveScratch) rmSync(work, { recursive: true, force: true }); }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 3) throw new Error('Usage: node scripts/rust-package-smoke.mjs PACKAGE_DIRECTORY');
  console.log(JSON.stringify(await smokePackage(process.argv[2]), null, 2));
}
