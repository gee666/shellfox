import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { delimiter, join, resolve } from 'node:path';
import { parseArgs } from 'node:util';
import { fileURLToPath } from 'node:url';
import { archives, fetchPinnedBytes, project, release, support, vendor, verifyArchive, verifyInputs, version, zigVersion } from './archives.mjs';

export const targets = Object.freeze({
  'x86_64-pc-windows-msvc': 'x86_64-windows-gnu',
  'x86_64-pc-windows-gnu': 'x86_64-windows-gnu',
  'aarch64-pc-windows-msvc': 'aarch64-windows-gnu',
  'aarch64-pc-windows-gnullvm': 'aarch64-windows-gnu',
  'x86_64-unknown-linux-gnu': 'x86_64-linux-gnu',
  'aarch64-unknown-linux-gnu': 'aarch64-linux-gnu',
  'x86_64-unknown-linux-musl': 'x86_64-linux-musl',
  'aarch64-unknown-linux-musl': 'aarch64-linux-musl',
  'x86_64-apple-darwin': 'x86_64-macos.13.0',
  'aarch64-apple-darwin': 'aarch64-macos.13.0',
  'aarch64-apple-ios': 'aarch64-ios.13.0',
  'aarch64-apple-ios-sim': 'aarch64-ios.13.0-simulator',
  'x86_64-apple-ios': 'x86_64-ios.13.0-simulator',
  'aarch64-linux-android': 'aarch64-linux-android.26',
  'armv7-linux-androideabi': 'arm-linux-androideabi.26',
  'i686-linux-android': 'x86-linux-android.26',
  'x86_64-linux-android': 'x86_64-linux-android.26',
});

export function assertZigVersion(actual) {
  if (actual.trim() !== zigVersion) {
    throw new Error(`Source build requires exact Zig ${zigVersion}; found ${actual.trim()}`);
  }
}

export function buildArgs(target, prefix, sourceDeps, cache, zigTarget) {
  if (!Object.hasOwn(targets, target)) throw new Error(`No source-build recipe for Rust target ${target}`);
  return ['build', '--prefix', prefix, '--system', sourceDeps, '--cache-dir', cache,
    '-Doptimize=ReleaseFast', '-Dcpu=baseline', `-Dtarget=${zigTarget ?? targets[target]}`];
}

export function sourceBuildEnv(scratch, globalCache, inherited = process.env) {
  const env = { ...inherited, ZIG_GLOBAL_CACHE_DIR: globalCache,
    GIT_CEILING_DIRECTORIES: [realpathSync(scratch), inherited.GIT_CEILING_DIRECTORIES]
      .filter(Boolean).join(delimiter) };
  // The release tarballs are not worktrees. Never let Git invoked by Zig find
  // Shellfox's parent repository or an explicitly inherited repository instead.
  for (const name of ['DESTDIR', 'GIT_DIR', 'GIT_WORK_TREE', 'GIT_COMMON_DIR',
    'GIT_INDEX_FILE', 'GIT_OBJECT_DIRECTORY', 'GIT_ALTERNATE_OBJECT_DIRECTORIES', 'GIT_NAMESPACE']) {
    delete env[name];
  }
  return env;
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { stdio: 'inherit', timeout: 120_000, ...options });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} failed with ${result.signal ?? result.status}`);
  return result;
}

function scratchDirectory() {
  const root = join(project, 'tmp');
  mkdirSync(root, { recursive: true });
  return mkdtempSync(join(root, 'ghostty-vt-prepare-'));
}

async function fetchArchives() {
  // The signed manifest and public key are retained in the repository.
  verifyInputs([]);
  mkdirSync(vendor, { recursive: true });
  const scratch = scratchDirectory();
  try {
    for (const name of Object.keys(archives)) {
      const destination = join(vendor, name);
      if (existsSync(destination)) {
        verifyArchive(name, readFileSync(destination));
        console.log(`Verified ${name}`);
        continue;
      }
      const data = await fetchPinnedBytes(`${release}${name}`);
      verifyArchive(name, data);
      const temporary = join(scratch, name);
      writeFileSync(temporary, data, { flag: 'wx' });
      renameSync(temporary, destination);
      console.log(`Acquired and verified ${name}`);
    }
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

function buildSource(options) {
  const target = options.target;
  if (!target || !options.prefix) throw new Error('build requires --target <Rust triple> and --prefix <new installation directory>');
  if (!Object.hasOwn(targets, target)) throw new Error(`No source-build recipe for Rust target ${target}`);
  const prefix = resolve(options.prefix);
  if (existsSync(prefix)) throw new Error(`Installation must not already exist: ${prefix}`);
  if (target.includes('android') && !process.env.ANDROID_NDK_HOME) {
    throw new Error('Set ANDROID_NDK_HOME to an exact installed NDK version; automatic latest-NDK selection is not reproducible');
  }
  const requestedZig = options.zig ?? 'zig';
  const zig = /[\\/]/.test(requestedZig) ? resolve(requestedZig) : requestedZig;
  const result = run(zig, ['version'], { encoding: 'utf8', timeout: 15_000, stdio: ['ignore', 'pipe', 'inherit'] });
  assertZigVersion(result.stdout);
  verifyInputs(['libghostty-vt-1.0.1.tar.gz', 'libghostty-vt-1.0.1-deps.tar.gz']);
  const scratch = scratchDirectory();
  let complete = false;
  try {
    const tar = process.platform === 'win32'
      ? join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe') : 'tar';
    for (const name of ['libghostty-vt-1.0.1.tar.gz', 'libghostty-vt-1.0.1-deps.tar.gz']) {
      run(tar, ['-xzf', join(vendor, name), '-C', scratch]);
    }
    const source = join(scratch, 'libghostty-vt-1.0.1');
    if (readFileSync(join(source, 'LIB_VERSION'), 'utf8').trim() !== version) {
      throw new Error('Source LIB_VERSION does not match pin');
    }
    const globalCache = join(scratch, 'global-cache');
    mkdirSync(globalCache);
    const env = sourceBuildEnv(scratch, globalCache);
    const args = buildArgs(target, prefix, join(scratch, 'libghostty-vt-1.0.1-deps'),
      join(scratch, 'local-cache'), options['zig-target']);
    run(zig, args, { cwd: source, env, timeout: 30 * 60_000 });
    for (const file of ['include/ghostty/vt.h', 'include/ghostty/vt/version.h',
      'lib/pkgconfig/libghostty-vt.pc', 'lib/pkgconfig/libghostty-vt-static.pc',
      ...(target.includes('windows') ? ['lib/ghostty-vt.lib', 'bin/ghostty-vt.dll'] : ['lib/libghostty-vt.a'])]) {
      if (!existsSync(join(prefix, file))) throw new Error(`Zig did not install ${file}`);
    }
    cpSync(join(support, 'notices'), join(prefix, 'share/ghostty-vt-notices'), { recursive: true });
    writeFileSync(join(prefix, 'ghostty-vt-target.txt'), `${target}\n`, { flag: 'wx' });
    writeFileSync(join(prefix, 'ghostty-vt-build.json'), `${JSON.stringify({
      version, zigVersion, rustTarget: target, zigTarget: options['zig-target'] ?? targets[target],
      archives, args, androidNdk: process.env.ANDROID_NDK_HOME ?? null,
      sdkroot: process.env.SDKROOT ?? null,
    }, null, 2)}\n`, { flag: 'wx' });
    complete = true;
    console.log(`Built ${version} for ${target}. Set GHOSTTY_VT_DIR=${prefix}`);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
    if (!complete) rmSync(prefix, { recursive: true, force: true });
  }
}

export async function main(args) {
  const { values, positionals } = parseArgs({ args, allowPositionals: true, options: {
    target: { type: 'string' }, prefix: { type: 'string' }, zig: { type: 'string' },
    'zig-target': { type: 'string' }, help: { type: 'boolean' },
  } });
  if (values.help || positionals.length === 0) {
    console.log(`Usage: node crates/ghostty-vt-sys/build-support/prepare.mjs <command>
  verify     Verify retained archives and the signed upstream checksum manifest
  fetch      Explicit network acquisition of missing, pinned archives into build-support/vendor
  build --target <Rust triple> --prefix <new install path> [--zig <executable>] [--zig-target <Zig triple>]
             Offline source build, requires exact Zig ${zigVersion}

Source recipes: ${Object.keys(targets).join(', ')}
Windows x86_64 MSVC prebuilt linking and Android arm64/API 24 source build/cross-check have been verified locally.
A successful recipe is not a cross-platform runtime test. See the crate README.`);
    return;
  }
  if (positionals.length !== 1) throw new Error('Expected exactly one command');
  switch (positionals[0]) {
    case 'verify':
      verifyInputs();
      console.log('All archive pins and upstream minisign signatures verified');
      break;
    case 'fetch': await fetchArchives(); break;
    case 'build': buildSource(values); break;
    default: throw new Error(`Unknown preparation command: ${positionals[0]}`);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch(error => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
