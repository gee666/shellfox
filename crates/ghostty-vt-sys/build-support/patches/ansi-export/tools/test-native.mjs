import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const { values } = parseArgs({ options: {
  source: { type: 'string' }, deps: { type: 'string' }, zig: { type: 'string' },
  profile: { type: 'string' }, out: { type: 'string' }, all: { type: 'boolean' }, keep: { type: 'boolean' },
} });
if (!values.source || !values.deps || !values.zig || !/^[0-9a-f]{64}$/.test(values.profile ?? '')) {
  throw new Error('Required: --source PATCHED_SOURCE --deps OFFLINE_DEPS --zig PINNED_ZIG --profile EXTERNAL_SHA256 [--all] [--out NEW_ROOT_TMP_DIR] [--keep]');
}
const here = dirname(fileURLToPath(import.meta.url));
const project = resolve(here, '../../../../../..');
const scratch = join(project, 'tmp'); mkdirSync(scratch, { recursive: true });
const out = values.out ? resolve(values.out) : mkdtempSync(join(scratch, 'ansi-native-'));
const rel = relative(scratch, out);
if (!rel || rel.startsWith('..') || isAbsolute(rel)) throw new Error('Output must be inside repository-root tmp');
if (values.out && existsSync(out)) throw new Error('Explicit output directory must not exist');
mkdirSync(out, { recursive: true });
const source = resolve(values.source), deps = resolve(values.deps), zig = resolve(values.zig);
const install = join(out, 'install');
const temporary = join(out, 'temporary'); mkdirSync(temporary, { recursive: true });
const env = { ...process.env, ZIG_GLOBAL_CACHE_DIR: join(out, 'global-cache'), GIT_CEILING_DIRECTORIES: dirname(source),
  TMP: temporary, TEMP: temporary, TMPDIR: temporary };
delete env.DESTDIR; delete env.GIT_DIR; delete env.GIT_WORK_TREE; delete env.GIT_COMMON_DIR;
function run(command, args, timeout = 360000) {
  const result = spawnSync(command, args, { encoding: 'utf8', env, cwd: out, timeout, maxBuffer: 64 * 1024 * 1024 });
  if (result.stdout) process.stdout.write(result.stdout);
  if (result.stderr) process.stderr.write(result.stderr);
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} exited ${result.status}`);
  return result.stdout;
}
try {
  if (run(zig, ['version'], 15000).trim() !== '0.17.0') throw new Error('Exact Zig 0.17.0 required');
  if (readFileSync(join(source, 'LIB_VERSION'), 'utf8').trim() !== '1.0.1') throw new Error('Wrong source release');
  const arch = process.arch === 'arm64' ? 'aarch64' : process.arch === 'x64' ? 'x86_64' : null;
  if (!arch) throw new Error('This host driver supports x64/arm64 only');
  const target = `${arch}-${process.platform === 'win32' ? 'windows-gnu' : process.platform === 'darwin' ? 'macos.13.0' : 'linux-gnu'}`;
  const args = ['--build-file', join(source, 'build.zig'), '--system', deps, '--cache-dir', join(out, 'local-cache'),
    '--prefix', install, `-Dtarget=${target}`, `-Doptimize=${values.all ? 'Debug' : 'ReleaseSafe'}`, '-Dcpu=baseline',
    '-Dversion-string=1.4.1-dev', '-Dlib-version-string=1.0.1', `-Dshellfox-engine-profile=${values.profile}`];
  run(zig, ['build', 'test-lib-vt-build', ...args, '-Demit-test-exe', ...(values.all ? [] : ['-Dtest-filter=Shellfox'])]);
  run(join(install, 'bin', `ghostty-vt-c-test${process.platform === 'win32' ? '.exe' : ''}`), [], 120000);
  const libraryArgs = args.map(arg => arg.startsWith('-Doptimize=') ? '-Doptimize=ReleaseSafe' : arg);
  run(zig, ['build', ...libraryArgs]);
  const smoke = join(install, 'bin', `shellfox-ansi-c-smoke${process.platform === 'win32' ? '.exe' : ''}`);
  const link = process.platform === 'win32' ? [join(install, 'lib', 'ghostty-vt.lib')] : ['-L', join(install, 'lib'), '-lghostty-vt'];
  run(zig, ['cc', `--target=${target}`, '-std=c11', '-Wall', '-Wextra', '-Werror', '-I', join(install, 'include'),
    resolve(here, '../tests/c-abi-smoke.c'), ...link, '-o', smoke], 120000);
  env.PATH = join(install, 'bin') + (process.platform === 'win32' ? ';' : ':') + (env.PATH ?? '');
  env.LD_LIBRARY_PATH = join(install, 'lib'); env.DYLD_LIBRARY_PATH = join(install, 'lib');
  run(smoke, [], 15000);
  console.log('Native checkpoint checks passed. This is not host/desktop/mobile acceptance or artifact authentication.');
} finally {
  if (!values.keep) rmSync(out, { recursive: true, force: true });
  else console.log(`Retained disposable checkpoint output: ${out}`);
}
