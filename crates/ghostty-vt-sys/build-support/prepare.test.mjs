import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync } from 'node:fs';
import { delimiter, join } from 'node:path';
import test from 'node:test';
import { archives, fetchPinnedBytes, project, vendor, verifyArchive, verifyInputs, verifyManifest } from './archives.mjs';
import { assertZigVersion, buildArgs, sourceBuildEnv, targets } from './prepare.mjs';

const manifest = readFileSync(join(vendor, 'SHA256SUMS'));
const signature = readFileSync(join(vendor, 'SHA256SUMS.minisig'), 'utf8');

test('retained official archives match pins and signed upstream manifest', () => {
  verifyInputs();
});

test('retries a transport timeout and a transient HTTP result before returning the bytes', async () => {
  let calls = 0;
  const waits = [];
  const data = await fetchPinnedBytes('https://example.invalid/pinned', {
    async request() {
      calls++;
      if (calls === 1) throw new DOMException('test timeout', 'TimeoutError');
      return calls === 2 ? new Response(null, { status: 503 }) : new Response('fixture');
    },
    async pause(ms) { waits.push(ms); },
  });
  assert.equal(data.toString(), 'fixture');
  assert.deepEqual(waits, [1000, 2000]);
});

test('does not retry a permanent download failure', async () => {
  let calls = 0;
  await assert.rejects(fetchPinnedBytes('https://example.invalid/pinned', {
    async request() { calls++; return new Response(null, { status: 404 }); },
    async pause() { assert.fail('unexpected retry'); },
  }), /HTTP 404/);
  assert.equal(calls, 1);
});

test('a corrupted archive is rejected before extraction', () => {
  const name = Object.keys(archives)[0];
  const data = readFileSync(join(vendor, name));
  data[Math.floor(data.length / 2)] ^= 1;
  assert.throws(() => verifyArchive(name, data), /SHA-256 mismatch/);
  assert.throws(() => verifyArchive('untrusted.zip', data), /Unpinned/);
});

test('both the manifest signature and trusted comment must authenticate', () => {
  const tampered = Buffer.from(manifest);
  tampered[0] ^= 1;
  assert.throws(() => verifyManifest(tampered, signature), /signature verification failed/);
  assert.throws(() => verifyManifest(manifest, signature.replace('timestamp:', 'timestamps:')),
    /signature verification failed/);
  const lines = signature.trimEnd().split(/\r?\n/);
  const packet = Buffer.from(lines[1], 'base64');
  packet[2] ^= 1;
  lines[1] = packet.toString('base64');
  assert.throws(() => verifyManifest(manifest, lines.join('\n')), /signing key/);
});

test('only exact Zig 0.17.0 is accepted', () => {
  assertZigVersion('0.17.0\r\n');
  for (const version of ['0.16.0', '0.17.1', '0.17.0-dev.1', '0.18.0']) {
    assert.throws(() => assertZigVersion(version), /requires exact Zig 0.17.0/);
  }
});

test('every source recipe uses offline dependency and fresh local cache directories', () => {
  for (const [target, zig] of Object.entries(targets)) {
    const args = buildArgs(target, '/install', '/pinned/deps', '/fresh/cache');
    assert.deepEqual(args.slice(0, 7),
      ['build', '--prefix', '/install', '--system', '/pinned/deps', '--cache-dir', '/fresh/cache']);
    assert.ok(args.includes(`-Dtarget=${zig}`));
    assert.ok(args.includes('-Dcpu=baseline'));
    assert.ok(args.includes('-Doptimize=ReleaseFast'));
    assert.ok(!args.some(arg => arg.startsWith('-fsys=')));
  }
});

test('source recipe refuses unsupported targets and allows explicit deployment targets', () => {
  assert.throws(() => buildArgs('mystery-target', '/install', '/deps', '/cache'), /No source-build recipe/);
  assert.ok(buildArgs('aarch64-linux-android', '/install', '/deps', '/cache', 'aarch64-linux-android.28')
    .includes('-Dtarget=aarch64-linux-android.28'));
});

test('source build confines Git discovery without changing the caller environment', () => {
  mkdirSync(join(project, 'tmp'), { recursive: true });
  const scratch = mkdtempSync(join(project, 'tmp', 'ghostty-git-env-test-'));
  try {
    const inherited = { DESTDIR: '/wrong', GIT_DIR: '/outside/.git',
      GIT_WORK_TREE: '/outside', GIT_COMMON_DIR: '/outside/.git', GIT_INDEX_FILE: '/outside/index',
      GIT_OBJECT_DIRECTORY: '/outside/objects', GIT_ALTERNATE_OBJECT_DIRECTORIES: '/other/objects',
      GIT_NAMESPACE: 'outside', GIT_CEILING_DIRECTORIES: '/retained-ceiling',
      ANDROID_NDK_HOME: '/pinned-ndk', SDKROOT: '/pinned-sdk', ZIG_GLOBAL_CACHE_DIR: '/old-cache' };
    const env = sourceBuildEnv(scratch, '/fresh-cache', inherited);
    assert.equal(env.GIT_CEILING_DIRECTORIES, `${realpathSync(scratch)}${delimiter}/retained-ceiling`);
    assert.equal(env.ZIG_GLOBAL_CACHE_DIR, '/fresh-cache');
    assert.equal(env.ANDROID_NDK_HOME, '/pinned-ndk');
    assert.equal(env.SDKROOT, '/pinned-sdk');
    for (const name of Object.keys(inherited).filter(name =>
      name === 'DESTDIR' || name.startsWith('GIT_') && name !== 'GIT_CEILING_DIRECTORIES')) {
      assert.equal(Object.hasOwn(env, name), false);
    }
    assert.equal(inherited.GIT_DIR, '/outside/.git');
    assert.equal(inherited.ZIG_GLOBAL_CACHE_DIR, '/old-cache');
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});

test('real Git cannot discover the parent repo from extracted source or dependencies', t => {
  const probe = spawnSync('git', ['--version'], { encoding: 'utf8', timeout: 15_000 });
  if (probe.error?.code === 'ENOENT') return t.skip('Git is not installed');
  assert.equal(probe.status, 0, probe.stderr);
  mkdirSync(join(project, 'tmp'), { recursive: true });
  const parent = mkdtempSync(join(project, 'tmp', 'ghostty-git-parent-test-'));
  try {
    const scratch = join(parent, 'extracted');
    const source = join(scratch, 'libghostty-vt-1.0.1');
    const deps = join(scratch, 'libghostty-vt-1.0.1-deps', 'dependency');
    for (const path of [source, deps]) mkdirSync(path, { recursive: true });
    const env = sourceBuildEnv(scratch, join(scratch, 'cache'));
    const init = spawnSync('git', ['init', '--quiet', parent], { env, encoding: 'utf8', timeout: 15_000 });
    assert.equal(init.status, 0, init.stderr);
    const unconfined = { ...env };
    delete unconfined.GIT_CEILING_DIRECTORIES;
    for (const cwd of [source, deps]) {
      const before = spawnSync('git', ['rev-parse', '--show-toplevel'],
        { cwd, env: unconfined, encoding: 'utf8', timeout: 15_000 });
      assert.equal(before.status, 0, before.stderr);
      const after = spawnSync('git', ['rev-parse', '--show-toplevel'],
        { cwd, env, encoding: 'utf8', timeout: 15_000 });
      assert.notEqual(after.status, 0);
      assert.match(after.stderr, /not a git repository/i);
    }
  } finally {
    rmSync(parent, { recursive: true, force: true });
  }
});
