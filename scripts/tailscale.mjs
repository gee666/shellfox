import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { env, root } from './common.mjs';

export const tailscaleVersion = '1.104.2';
export const goVersion = '1.27.3';
const platforms = { win32: 'windows', darwin: 'darwin', linux: 'linux' };
const architectures = { x64: 'amd64', arm64: 'arm64' };
const source = path.join(root, 'native/tailscale');

export function helperTarget(platform = process.platform, arch = process.arch) {
  if (!Object.hasOwn(platforms, platform) || !Object.hasOwn(architectures, arch)) throw new Error('Tailscale helper requires Windows, macOS or Linux x64/arm64.');
  return { goos: platforms[platform], goarch: architectures[arch], basename: 'shellfox-tailscale' + (platform === 'win32' ? '.exe' : ''),
    directory: path.join(root, 'tmp/tailscale', `${platform}-${arch}`) };
}

function goEnvironment(target) {
  const scratch = path.join(root, 'tmp');
  for (const directory of ['go-cache', 'go-mod', 'go-work']) mkdirSync(path.join(scratch, directory), { recursive: true });
  return { ...env, CGO_ENABLED: '0', GOTOOLCHAIN: 'local', GOENV: 'off', GOFLAGS: '', GOWORK: 'off',
    GOOS: target.goos, GOARCH: target.goarch, GOCACHE: path.join(scratch, 'go-cache'),
    GOMODCACHE: path.join(scratch, 'go-mod'), GOTMPDIR: path.join(scratch, 'go-work') };
}

function go(args, target, capture = false) {
  const command = process.env.SHELLFOX_GO || 'go';
  const result = spawnSync(command, args, { cwd: source, env: goEnvironment(target), timeout: 600000, windowsHide: true,
    encoding: 'utf8', stdio: capture ? 'pipe' : 'inherit', maxBuffer: 32 * 1024 * 1024 });
  if (result.error || result.status !== 0) throw new Error(`Go helper command failed: ${args[0]}. Install Go ${goVersion}+ or set SHELLFOX_GO to its executable. ${result.error?.message ?? result.stderr ?? ''}`);
  return result.stdout;
}

function notices(target) {
  // Only modules in this target's compiled package graph belong in its notices.
  const modules = new Map();
  const rows = go(['list', '-mod=readonly', '-deps', '-f', '{{with .Module}}{{.Path}}\t{{.Version}}\t{{.Dir}}{{end}}', '.'], target, true);
  for (const row of rows.split(/\r?\n/)) {
    const [name, version, directory] = row.split('\t');
    if (name && version && directory) modules.set(name, { version, directory });
  }
  let text = `Shellfox embedded Tailscale helper\nTailscale v${tailscaleVersion}, https://github.com/tailscale/tailscale\n\n`;
  const missing = [];
  for (const [name, { version, directory }] of [...modules].sort(([a], [b]) => a.localeCompare(b))) {
    text += `\n===== ${name} ${version} =====\n`;
    const files = readdirSync(directory, { withFileTypes: true }).filter(entry => entry.isFile() && /^(licen[sc]e|copying|notice|copyright)([._-].*)?$/i.test(entry.name));
    if (!files.length) { missing.push(name); text += 'No module-root license file found. See NOTICE-AUDIT.md.\n'; }
    for (const entry of files) text += `\n--- ${entry.name} ---\n${readFileSync(path.join(directory, entry.name), 'utf8')}\n`;
  }
  const goroot = go(['env', 'GOROOT'], target, true).trim();
  text += `\n===== Go standard library =====\n${readFileSync(path.join(goroot, 'LICENSE'), 'utf8')}\n`;
  writeFileSync(path.join(target.directory, 'THIRD-PARTY-NOTICES.txt'), text);
  copyFileSync(path.join(source, 'TAILSCALE-LICENSE'), path.join(target.directory, 'TAILSCALE-LICENSE'));
  copyFileSync(path.join(source, 'NOTICE-AUDIT.md'), path.join(target.directory, 'NOTICE-AUDIT.md'));
  writeFileSync(path.join(target.directory, 'modules.json'), JSON.stringify({ tailscaleVersion, modules: [...modules].map(([name, { version }]) => ({ name, version })), missingModuleRootLicenses: missing }, null, 2) + '\n');
}

export function probeHelper(executable, platform = process.platform, arch = process.arch) {
  const target = helperTarget(platform, arch);
  const result = spawnSync(executable, ['--probe'], { env, timeout: 10000, windowsHide: true, encoding: 'utf8' });
  assert.equal(result.error, undefined, 'Cannot execute embedded Tailscale helper');
  assert.equal(result.status, 0, result.stderr);
  const probe = JSON.parse(result.stdout);
  assert.deepEqual(probe, { arch: target.goarch, platform: target.goos, protocolVersion: 1, tailscaleVersion });
  return probe;
}

export function buildHelper(platform = process.platform, arch = process.arch) {
  const target = helperTarget(platform, arch);
  mkdirSync(target.directory, { recursive: true });
  const executable = path.join(target.directory, target.basename), partial = executable + '.part';
  rmSync(partial, { force: true });
  try {
    go(['build', '-mod=readonly', '-trimpath', '-buildvcs=false', '-ldflags=-s -w', '-o', partial, '.'], target);
    chmodSync(partial, 0o755);
    if (platform === process.platform && arch === process.arch) probeHelper(partial, platform, arch);
    notices(target);
    renameSync(partial, executable);
  } finally { rmSync(partial, { force: true }); }
  console.log(`[tailscale] ${platform}-${arch}: ${executable}`);
  return executable;
}

async function testHelper() {
  const target = helperTarget();
  go(['test', '-mod=readonly', '-count=1', './...'], target);
  const executable = buildHelper();
  // Exercise the real process and framing without calling start or authenticating.
  const input = [
    { id: 1, method: 'status' },
    { id: 'invalid', method: 'start', payload: { authKey: 'tskey-auth-DO-NOT-ECHO' } },
    { id: 2, method: 'stop' },
  ].map(message => JSON.stringify(message)).join('\n') + '\n';
  const result = spawnSync(executable, [], { input, env, timeout: 10000, encoding: 'utf8', windowsHide: true });
  assert.equal(result.status, 0, result.stderr);
  assert.ok(!result.stdout.includes('DO-NOT-ECHO') && !result.stderr.includes('DO-NOT-ECHO'));
  const responses = result.stdout.trim().split(/\r?\n/).map(line => JSON.parse(line));
  assert.deepEqual(responses.map(({ id, ok }) => ({ id, ok })), [{ id: 1, ok: false }, { id: 'invalid', ok: false }, { id: 2, ok: true }]);
  assert.deepEqual(responses[2].value, { stopped: true });
  await ownershipSmoke(executable);
}

async function ownershipSmoke(executable) {
  const directory = mkdtempSync(path.join(root, 'tmp/ownership-smoke-'));
  const child = spawn(executable, ['--ownership', directory], { env, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
  let stdout = '', stderr = '';
  child.stderr.on('data', data => { stderr += data.toString(); });
  const exited = new Promise((resolve, reject) => { child.once('error', reject); child.once('exit', resolve); });
  const ready = new Promise((resolve, reject) => {
    child.once('error', reject);
    child.stdout.on('data', data => {
      stdout += data.toString();
      if (stdout.includes('\n')) resolve();
    });
    child.once('exit', () => reject(new Error('Ownership helper exited before acknowledgement.')));
  });
  const deadline = setTimeout(() => child.kill(), 10000);
  try {
    await ready;
    assert.equal(stdout, '{"locked":true}\n');
    const contender = spawnSync(executable, ['--ownership', directory], { env, input: '', timeout: 10000, encoding: 'utf8', windowsHide: true });
    assert.equal(contender.status, 0, contender.stderr);
    assert.equal(contender.stdout, '{"locked":false}\n');
    assert.equal(contender.stderr, '');
    child.stdin.end();
    assert.equal(await exited, 0);
    assert.equal(stdout, '{"locked":true}\n');
    assert.equal(stderr, '');
    const next = spawnSync(executable, ['--ownership', directory], { env, input: '', timeout: 10000, encoding: 'utf8', windowsHide: true });
    assert.equal(next.status, 0, next.stderr);
    assert.equal(next.stdout, '{"locked":true}\n');
    assert.equal(next.stderr, '');
    const invalid = spawnSync(executable, ['--ownership'], { env, input: '', timeout: 10000, encoding: 'utf8', windowsHide: true });
    assert.equal(invalid.status, 2);
    assert.deepEqual(JSON.parse(invalid.stdout), { error: 'Invalid ownership arguments' });
    assert.equal(invalid.stderr, '');
  } finally {
    clearTimeout(deadline);
    if (child.exitCode === null && child.signalCode === null) { child.kill(); await exited.catch(() => {}); }
    rmSync(directory, { recursive: true, force: true });
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command = 'build', ...args] = process.argv.slice(2);
  const options = Object.fromEntries(args.map(arg => {
    const match = /^--(platform|arch|executable)=(.+)$/.exec(arg);
    if (!match) throw new Error('Unknown helper option: ' + arg);
    return [match[1], match[2]];
  }));
  if (command === 'build') buildHelper(options.platform, options.arch);
  else if (command === 'test' && !args.length) await testHelper();
  else if (command === 'probe' && options.executable && existsSync(options.executable)) probeHelper(path.resolve(options.executable), options.platform, options.arch);
  else throw new Error('Usage: node scripts/tailscale.mjs build [--platform=win32|linux|darwin] [--arch=x64|arm64], test, or probe --executable=PATH');
}
