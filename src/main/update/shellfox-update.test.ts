import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import path from 'node:path';
import { createServer, type Server } from 'node:http';
import { chmod, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import { execFile, spawnSync } from 'node:child_process';
import type { AddressInfo } from 'node:net';

const script = path.resolve('src/main/update/shellfox-update.sh');
const enabled = process.platform === 'linux' && spawnSync('curl', ['--version']).status === 0;
let root = '', bin = '', log = '', server: Server, notFoundUrl = '';
const fake = (name: string, body: string) => writeFile(path.join(bin, name), '#!/bin/sh\n' + body + '\n').then(() => chmod(path.join(bin, name), 0o755));
interface Run { code: number; out: string; err: string; calls: string[]; leftover: string[] }
async function run(args: string[], options: { installed?: string | null; api?: string; arch?: string; uid?: string } = {}): Promise<Run> {
  const scratch = await mkdtemp(path.join(root, 'run-')), tmp = path.join(scratch, 'tmp');
  await mkdir(tmp); await writeFile(log, '');
  await fake('dpkg-query', options.installed === null ? 'exit 1' : `printf '%s' '${options.installed ?? '0.1.1'}'`);
  await fake('dpkg', `echo "dpkg $*" >> "$LOG"; [ "$1" = --print-architecture ] && echo ${options.arch ?? 'amd64'}; exit 0`);
  await fake('id', `echo ${options.uid ?? '1000'}`);
  await fake('sudo', 'echo "sudo $*" >> "$LOG"; exec "$@"');
  await fake('apt-get', 'echo "apt-get $*" >> "$LOG"; for f; do case $f in /*) [ -r "$f" ] && echo "readable $(basename "$f") $(stat -c %a "$f") dir $(stat -c %a "$(dirname "$f")")" >> "$LOG";; esac; done; exit 0');
  const env = { ...process.env, PATH: bin + ':' + process.env.PATH, LOG: log, TMPDIR: tmp, SHELLFOX_UPDATE_API: options.api ?? '' };
  const result = await new Promise<Run>(resolve => execFile('/bin/sh', [script, ...args], { env, timeout: 20000 }, async (error, out, err) => {
    resolve({ code: error ? (error as NodeJS.ErrnoException & { code: number }).code : 0, out, err, calls: (await readFile(log, 'utf8')).split('\n').filter(Boolean), leftover: await readdir(tmp) });
  }));
  return result;
}
async function release(name: string, tag: string, assets: string[], pretty = true): Promise<string> {
  const dir = path.join(root, name); await mkdir(dir, { recursive: true });
  const list = [];
  for (const asset of assets) { await writeFile(path.join(dir, asset), 'fixture ' + asset); list.push({ name: asset, url: 'https://api.invalid/assets/1', browser_download_url: pathToFileURL(path.join(dir, asset)).href }); }
  const file = path.join(dir, 'release.json');
  await writeFile(file, JSON.stringify({ url: 'https://api.invalid/r', tag_name: tag, name: tag, assets: list }, null, pretty ? 2 : undefined));
  return pathToFileURL(file).href;
}
beforeAll(async () => {
  if (!enabled) return;
  await mkdir(path.resolve('tmp'), { recursive: true });
  root = await mkdtemp(path.resolve('tmp/shellfox-update-')); bin = path.join(root, 'bin'); log = path.join(root, 'calls.log'); await mkdir(bin);
  server = createServer((_request, response) => { response.statusCode = 404; response.end('{"message":"Not Found"}'); });
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve)); notFoundUrl = `http://127.0.0.1:${(server.address() as AddressInfo).port}/latest`;
});
afterAll(async () => { if (!enabled) return; await new Promise(resolve => server.close(resolve)); await rm(root, { recursive: true, force: true }); });

describe.skipIf(!enabled)('shellfox-update.sh (Linux .deb)', () => {
  const assets = ['shellfox_0.2.0_amd64.deb', 'shellfox_0.2.0_arm64.deb', 'Shellfox-linux-x64-0.2.0.zip'];
  it('has valid sh syntax and prints usage', async () => {
    expect(spawnSync('/bin/sh', ['-n', script]).status).toBe(0);
    const result = await run(['--help']); expect(result.code).toBe(0); expect(result.out).toContain('Usage: shellfox update [--check]');
    expect((await run(['--bogus'])).code).toBe(2);
  });
  it('says up to date and does nothing when the release is not newer', async () => {
    for (const [installed, tag] of [['0.2.0', 'v0.2.0'], ['0.3.0-1', 'v0.2.0']] as const) {
      const result = await run([], { installed, api: await release('same', tag, assets) });
      expect(result.code).toBe(0); expect(result.out).toContain('up to date'); expect(result.calls).toEqual([]); expect(result.leftover).toEqual([]);
    }
  });
  it('--check reports an available update without downloading or installing', async () => {
    const result = await run(['--check'], { api: await release('check', 'v0.2.0', assets, false) });
    expect(result.code).toBe(0); expect(result.out).toContain('Shellfox 0.2.0 is available (installed: 0.1.1). Run: shellfox update');
    expect(result.calls).toEqual([]); expect(result.leftover).toEqual([]);
    expect((await run(['--check'], { installed: '0.2.0', api: await release('check2', 'v0.2.0', assets) })).out).toContain('up to date');
  });
  it('downloads the amd64 deb and installs it with sudo apt-get', async () => {
    const result = await run([], { installed: '0.1.1-1', api: await release('full', 'v0.2.0', assets) });
    expect(result.code).toBe(0);
    const install = result.calls.find(call => call.startsWith('sudo apt-get'))!;
    expect(install).toMatch(/^sudo apt-get install -y \/.+\/shellfox-update\.[^/]+\/shellfox_0\.2\.0_amd64\.deb$/);
    expect(result.calls).toContain('apt-get ' + install.slice('sudo apt-get '.length));
    expect(result.calls.some(call => /^readable shellfox_0\.2\.0_amd64\.deb 644 dir 755$/.test(call))).toBe(true);
    expect(result.out).toContain('Restart Shellfox to use 0.2.0 (quitting Shellfox closes its terminals).');
    expect(result.leftover).toEqual([]);
  });
  it('selects the arm64 deb on arm64 and skips sudo as root', async () => {
    const result = await run([], { arch: 'arm64', uid: '0', api: await release('arm', 'v0.2.0', assets) });
    expect(result.code).toBe(0); expect(result.calls.some(call => call.startsWith('sudo'))).toBe(false);
    expect(result.calls.find(call => call.startsWith('apt-get install -y '))).toMatch(/shellfox_0\.2\.0_arm64\.deb$/);
  });
  it('explains and fails when not installed from the .deb', async () => {
    const result = await run([], { installed: null, api: await release('nodeb', 'v0.2.0', assets) });
    expect(result.code).toBe(1); expect(result.err).toContain('not installed from the .deb'); expect(result.err).toContain('https://github.com/gee666/shellfox/releases');
    expect(result.calls).toEqual([]);
  });
  it('fails clearly when the release lacks the package for this architecture', async () => {
    const result = await run([], { api: await release('noasset', 'v0.2.0', ['shellfox_0.2.0_arm64.deb']) });
    expect(result.code).toBe(1); expect(result.err).toContain('no file named shellfox_0.2.0_amd64.deb'); expect(result.calls.filter(call => /apt-get|sudo|dpkg -i/.test(call))).toEqual([]); expect(result.leftover).toEqual([]);
  });
  it('fails when the install command fails and still cleans up', async () => {
    const api = await release('aptfail', 'v0.2.0', assets);
    await fake('apt-get', 'exit 100');
    const scratch = await mkdtemp(path.join(root, 'fail-')); await mkdir(path.join(scratch, 'tmp'));
    const env = { ...process.env, PATH: bin + ':' + process.env.PATH, LOG: log, TMPDIR: path.join(scratch, 'tmp'), SHELLFOX_UPDATE_API: api };
    const code = await new Promise<number>(resolve => execFile('/bin/sh', [script], { env }, error => resolve(error ? (error as unknown as { code: number }).code : 0)));
    expect(code).toBe(1); expect(await readdir(path.join(scratch, 'tmp'))).toEqual([]);
  });
  it('reports no published release on 404, and failure when unreachable or malformed', async () => {
    const none = await run([], { api: notFoundUrl }); expect(none.code).toBe(0); expect(none.out).toContain('No published Shellfox release yet'); expect(none.calls).toEqual([]);
    expect((await run(['--check'], { api: notFoundUrl })).out).toContain('No published Shellfox release yet');
    for (const api of ['http://127.0.0.1:1/latest', pathToFileURL(path.join(root, 'missing.json')).href]) {
      const result = await run([], { api }); expect(result.code).toBe(1); expect(result.err).toContain('could not query the latest release');
    }
    const bad = path.join(root, 'bad.json'); await writeFile(bad, '{"message":"rate limited"}');
    const malformed = await run([], { api: pathToFileURL(bad).href }); expect(malformed.code).toBe(1); expect(malformed.err).toContain('did not contain a version');
  });
});

// macOS flow exercised on Linux with fake uname/PlistBuddy/lipo/ditto/xattr (the "zip" fixture is a tar file).
describe.skipIf(!enabled)('shellfox-update.sh (macOS bundle)', () => {
  async function bundle(parent: string, version: string, name = 'Shellfox.app') {
    const app = path.join(parent, name); await mkdir(path.join(app, 'Contents/MacOS'), { recursive: true });
    await writeFile(path.join(app, 'Contents/Info.plist'), `id=local.shellfox\nversion=${version}\n`);
    await writeFile(path.join(app, 'Contents/MacOS/Shellfox'), 'binary ' + version); return app;
  }
  async function macRun(args: string[], options: { installed?: string; newVersion?: string; tag?: string; dittoFails?: boolean; bundleId?: string } = {}) {
    const scratch = await mkdtemp(path.join(root, 'mac-')), macBin = path.join(scratch, 'bin'), apps = path.join(scratch, 'Applications'), tmp = path.join(scratch, 'tmp'), callLog = path.join(scratch, 'calls.log');
    await mkdir(macBin); await mkdir(apps); await mkdir(tmp); await writeFile(callLog, '');
    const app = await bundle(apps, options.installed ?? '0.1.1');
    if (options.bundleId) await writeFile(path.join(app, 'Contents/Info.plist'), `id=${options.bundleId}\nversion=0.1.1\n`);
    const staging = await bundle(path.join(scratch, 'staging'), options.newVersion ?? '0.2.0'), assetDir = path.join(scratch, 'assets'); await mkdir(assetDir);
    const zip = path.join(assetDir, 'Shellfox-darwin-arm64-0.2.0.zip');
    spawnSync('tar', ['-cf', zip, '-C', path.dirname(staging), 'Shellfox.app']);
    const api = path.join(assetDir, 'release.json');
    await writeFile(api, JSON.stringify({ tag_name: options.tag ?? 'v0.2.0', assets: [{ name: path.basename(zip), browser_download_url: pathToFileURL(zip).href }, { name: 'Shellfox-darwin-x64-0.2.0.zip', browser_download_url: 'file:///nonexistent/x64.zip' }] }, null, 2));
    const put = async (name: string, body: string) => { await writeFile(path.join(macBin, name), '#!/bin/sh\n' + body + '\n'); await chmod(path.join(macBin, name), 0o755); };
    await put('uname', 'case "$1" in -s) echo Darwin;; -m) echo arm64;; *) echo Darwin;; esac');
    await put('PlistBuddy', 'case "$2" in *Identifier) sed -n "s/^id=//p" "$3";; *ShortVersion*) sed -n "s/^version=//p" "$3";; esac');
    await put('lipo', 'echo arm64');
    await put('xattr', 'echo "xattr $*" >> "$LOG"');
    await put('sudo', 'echo "sudo $*" >> "$LOG"; exec "$@"');
    await put('ditto', options.dittoFails ? 'case "$1" in -x) tar -xf "$3" -C "$4";; *) exit 1;; esac' : 'case "$1" in -x) tar -xf "$3" -C "$4";; *) cp -R "$1" "$2";; esac');
    const env = { ...process.env, PATH: macBin + ':' + process.env.PATH, LOG: callLog, TMPDIR: tmp, SHELLFOX_UPDATE_API: pathToFileURL(api).href, SHELLFOX_PLISTBUDDY: path.join(macBin, 'PlistBuddy'), SHELLFOX_APP_EXECUTABLE: path.join(app, 'Contents/MacOS/Shellfox') };
    const result = await new Promise<{ code: number; out: string; err: string }>(resolve => execFile('/bin/sh', [script, ...args], { env, timeout: 20000 }, (error, out, err) => resolve({ code: error ? (error as unknown as { code: number }).code : 0, out, err })));
    return { ...result, installed: await readFile(path.join(app, 'Contents/MacOS/Shellfox'), 'utf8'), apps: await readdir(apps), calls: (await readFile(callLog, 'utf8')).split('\n').filter(Boolean), leftover: await readdir(tmp) };
  }
  it('replaces the app bundle in place, removes quarantine and cleans up', async () => {
    const result = await macRun([]);
    expect(result.code).toBe(0); expect(result.out).toContain('Restart Shellfox to use 0.2.0 (quitting Shellfox closes its terminals).');
    expect(result.installed).toBe('binary 0.2.0'); expect(result.apps).toEqual(['Shellfox.app']); expect(result.leftover).toEqual([]);
    expect(result.calls.some(call => /^xattr -dr com\.apple\.quarantine .*Shellfox\.app$/.test(call))).toBe(true); expect(result.calls.some(call => call.startsWith('sudo'))).toBe(false);
  });
  it('--check and up-to-date never touch the bundle', async () => {
    const check = await macRun(['--check']); expect(check.out).toContain('Shellfox 0.2.0 is available'); expect(check.installed).toBe('binary 0.1.1'); expect(check.leftover).toEqual([]);
    const same = await macRun([], { installed: '0.2.0', newVersion: '0.2.0' }); expect(same.out).toContain('up to date'); expect(same.installed).toBe('binary 0.2.0');
  });
  it('keeps the previous version when installing the new one fails', async () => {
    const result = await macRun([], { dittoFails: true });
    expect(result.code).toBe(1); expect(result.installed).toBe('binary 0.1.1'); expect(result.apps).toEqual(['Shellfox.app']); expect(result.leftover).toEqual([]);
  });
  it('refuses a bundle that is not Shellfox', async () => {
    const result = await macRun([], { bundleId: 'com.github.Electron' });
    expect(result.code).toBe(1); expect(result.err).toContain('not a Shellfox app bundle'); expect(result.installed).toBe('binary 0.1.1');
  });
});
