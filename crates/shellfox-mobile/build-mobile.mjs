import { existsSync, readFileSync, mkdtempSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { androidTargets, beginAndroidOutput, finishAndroidOutput, stageAndroidAbi, sha256 } from './android-artifacts.mjs';

export const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const android = Object.fromEntries(Object.values(androidTargets).map(value => [value.target, [value.zig, value.clang]]));
const apple = {
  'aarch64-apple-ios': ['aarch64', 'iphoneos', ''],
  'aarch64-apple-ios-sim': ['aarch64', 'iphonesimulator', '-simulator'],
  'x86_64-apple-ios': ['x86_64', 'iphonesimulator', '-simulator'],
};
export function minimums(project = root) {
  const pkg = JSON.parse(readFileSync(join(project, 'mobile/package.json'), 'utf8'));
  const rn = JSON.parse(readFileSync(join(project, 'node_modules/react-native/package.json'), 'utf8'));
  if (pkg.dependencies['react-native'] !== '0.88.0' || rn.version !== '0.88.0') {
    throw new Error('Reconcile the native recipe before changing React Native 0.88.0.');
  }
  const gradle = readFileSync(join(project, 'mobile/android/build.gradle'), 'utf8');
  if (!/minSdkVersion\s*=\s*24\b/.test(gradle)) throw new Error('Android API 24 contract changed.');
  if (!/ndkVersion\s*=\s*"27\.1\.12297006"/.test(gradle)) throw new Error('NDK pin changed.');
  const helper = readFileSync(join(project, 'node_modules/react-native/scripts/cocoapods/helpers.rb'), 'utf8');
  const ios = helper.match(/def self\.min_ios_version_supported\s+return '([0-9.]+)'/)?.[1];
  if (!ios || !/^\d+\.\d+(?:\.\d+)?$/.test(ios)) throw new Error('Cannot resolve effective RN iOS minimum.');
  const podfile = readFileSync(join(project, 'mobile/ios/Podfile'), 'utf8');
  if (!/platform :ios, min_ios_version_supported/.test(podfile)) throw new Error('Reconcile Podfile minimum.');
  return { android: 24, ios, ndk: '27.1.12297006' };
}
export function plan(target, project = root) {
  const minimum = minimums(project);
  const prefix = join(project, 'tmp/mobile-rust-build', target, 'ghostty');
  if (Object.hasOwn(android, target)) {
    return { target, prefix, zigTarget: android[target][0], clang: android[target][1], minimum };
  }
  if (Object.hasOwn(apple, target)) {
    const [arch, sdk, suffix] = apple[target];
    return { target, prefix, zigTarget: `${arch}-ios.${minimum.ios}${suffix}`, sdk, minimum };
  }
  throw new Error('Expected an Android or iOS Rust target.');
}
function run(command, args, env = process.env, timeout = 120_000) {
  const result = spawnSync(command, args, { cwd: root, env, stdio: 'inherit', timeout });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} failed with ${result.status ?? result.signal}`);
}
function capture(command, args) {
  const result = spawnSync(command, args, { cwd: root, encoding: 'utf8', timeout: 15_000 });
  if (result.error || result.status !== 0) throw new Error(`Cannot run ${command}`);
  return result.stdout.trim();
}
function buildCapture(command, args, env) {
  const result = spawnSync(command, args, { cwd: root, env, timeout: 1_800_000, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  if (result.stdout) process.stdout.write(result.stdout); if (result.stderr) process.stderr.write(result.stderr);
  if (result.error || result.status !== 0) throw new Error(`${command} failed: ${result.error ?? result.status}`);
  return `${result.stdout ?? ''}\n${result.stderr ?? ''}`;
}
export function build(recipe, zig, options = {}) {
  const env = { ...process.env, GHOSTTY_VT_DIR: recipe.prefix, GHOSTTY_VT_LINK_KIND: 'static' };
  let ndkBin;
  if (options.targetDir) env.CARGO_TARGET_DIR = options.targetDir;
  if (options.staging) {
    if (env.GHOSTTY_VT_ENGINE_PROFILE || env.CARGO_ENCODED_RUSTFLAGS || env.RUSTFLAGS) {
      throw new Error('Default Android producer requires reviewed target flags only; experimental/inherited global engine flags need a separate coordinated build.');
    }
    env.CARGO_NET_OFFLINE = 'true'; env.CARGO_BUILD_JOBS = '1';
  }
  if (recipe.clang) {
    const ndk = process.env.ANDROID_NDK_HOME;
    if (!ndk || !existsSync(join(ndk, 'source.properties'))) throw new Error('Set ANDROID_NDK_HOME to NDK 27.1.12297006.');
    const properties = readFileSync(join(ndk, 'source.properties'), 'utf8');
    if (!/Pkg.Revision\s*=\s*27\.1\.12297006\b/.test(properties)) throw new Error('Wrong NDK version.');
    const host = { win32: 'windows-x86_64', darwin: 'darwin-x86_64', linux: 'linux-x86_64' }[process.platform];
    if (!host) throw new Error('Unsupported NDK build host.');
    const bin = join(ndk, 'toolchains/llvm/prebuilt', host, 'bin'); ndkBin = bin;
    // Invoke clang itself with an explicit API target. Windows .cmd target
    // wrappers are not executable linkers for Rust's CreateProcess invocation.
    const clang = join(bin, 'clang' + (process.platform === 'win32' ? '.exe' : ''));
    if (!existsSync(clang)) throw new Error('Pinned NDK clang is missing.');
    const clangTarget = recipe.clang.replace(/-clang$/, '');
    const cargoTarget = recipe.target.toUpperCase().replaceAll('-', '_');
    const ccTarget = recipe.target.replaceAll('-', '_');
    env[`CARGO_TARGET_${cargoTarget}_LINKER`] = clang;
    env[`CARGO_TARGET_${cargoTarget}_RUSTFLAGS`] = [
      env[`CARGO_TARGET_${cargoTarget}_RUSTFLAGS`], `-C link-arg=--target=${clangTarget}`,
      options.staging ? '-C link-arg=-Wl,-z,max-page-size=16384' : '',
    ].filter(Boolean).join(' ');
    env[`CC_${ccTarget}`] = clang;
    env[`CFLAGS_${ccTarget}`] = [env[`CFLAGS_${ccTarget}`], `--target=${clangTarget}`].filter(Boolean).join(' ');
    env[`AR_${ccTarget}`] = join(bin, 'llvm-ar' + (process.platform === 'win32' ? '.exe' : ''));
  } else {
    if (process.platform !== 'darwin') throw new Error('iOS native builds require macOS and Xcode.');
    env.SDKROOT = capture('xcrun', ['--sdk', recipe.sdk, '--show-sdk-path']);
    env.IPHONEOS_DEPLOYMENT_TARGET = recipe.minimum.ios;
  }
  // No fetch/install fallback. The pinned source/dependency archives must exist.
  if (!options.prepared) run(process.execPath, [
    'crates/ghostty-vt-sys/build-support/prepare.mjs', 'build',
    '--target', recipe.target, '--prefix', recipe.prefix, '--zig', zig,
    '--zig-target', recipe.zigTarget,
    ...(options.native002 ? ['--experimental-shellfox-native', ...(options.patch ? ['--patch', options.patch] : [])] : []),
  ], env, 1_800_000);
  let nativeVt;
  if (options.native002) {
    const file = join(recipe.prefix, 'shellfox-native002.json');
    const vt = JSON.parse(readFileSync(file, 'utf8'));
    const built = JSON.parse(readFileSync(join(recipe.prefix, 'ghostty-vt-build.json'), 'utf8'));
    if (vt.schema !== 1 || vt.status !== 'experimental-native002-not-final-acceptance' ||
      vt.profileIsAuthenticity !== false || built.rustTarget !== recipe.target || built.zigTarget !== recipe.zigTarget ||
      createHash('sha256').update(vt.recipe ?? '').digest('hex') !== vt.engineProfile ||
      !Array.isArray(vt.artifacts) || !vt.artifacts.length || vt.artifacts.some(a =>
        typeof a.path !== 'string' || !/^(include|lib)\/[A-Za-z0-9_./-]+$/.test(a.path) || a.path.includes('..') || sha256(join(recipe.prefix, a.path)) !== a.sha256))
      throw new Error('Prepared Native002 target/recipe/artifact verification failed.');
    env.GHOSTTY_VT_ENGINE_PROFILE = vt.engineProfile;
    nativeVt = { engineProfile: vt.engineProfile, manifestSha256: sha256(file), artifacts: vt.artifacts,
      profileIsAuthenticity: false, runtimeAcceptance: false };
  }
  if (!options.staging) {
    run('cargo', ['build', '--locked', '--offline', '-p', 'shellfox-mobile', '--release', '--target', recipe.target], env, 1_800_000);
    return;
  }
  const args = ['rustc', '--locked', '--offline', '--lib', '-j', '1', '-p', 'shellfox-mobile', '--release', '--target', recipe.target,
    ...(options.native002 ? ['--features', 'shellfox-mobile/experimental-shellfox-native'] : []), '--', '--print', 'native-static-libs'];
  const output = buildCapture('cargo', args, env);
  const nativeStaticLibraries = output.match(/native-static-libs:\s*([^\r\n]+)/)?.[1];
  if (!nativeStaticLibraries) throw new Error('Rust did not report its actual native linker requirements.');
  const archive = join(options.targetDir, recipe.target, 'release/libshellfox_mobile.a');
  const tool = join(ndkBin, `llvm-nm${process.platform === 'win32' ? '.exe' : ''}`);
  if (!existsSync(tool)) throw new Error('Pinned NDK llvm-nm is missing; no symbol measurement ran.');
  const nmArgs = ['--defined-only', archive];
  const symbols = buildCapture(tool, nmArgs, env).split(/\r?\n/).map(line => line.trim().split(/\s+/).at(-1))
    .filter(name => /^sfm_[a-z0-9_]+$/.test(name ?? ''));
  return { archive, command: ['cargo', ...args], nativeStaticLibraries, nativeVt,
    measurement: { kind: 'ndk-llvm-nm', tool, command: [tool, ...nmArgs], functions: [...new Set(symbols)].sort() } };
}
export function buildAndroid(zig, outputRoot, options = {}) {
  if (options.native002 && !outputRoot) throw new Error('Native002 requires a distinct explicit output root, never the default selector.');
  const batch = beginAndroidOutput(root, outputRoot, options);
  // Short fresh target avoids Windows/NDK MAX_PATH without touching old targets.
  batch.work = mkdtempSync(join(root, 'tmp', 'mma-'));
  // Sequential ABIs, bounded Cargo workers. Partial/mixed sets never become current.
  for (const [abi, descriptor] of Object.entries(androidTargets)) {
    const prepared = options.prepared?.[abi];
    const recipe = { ...plan(descriptor.target), prefix: prepared ? resolve(prepared) : join(batch.work, 'vt', abi) };
    const output = build(recipe, zig, { ...options, prepared: Boolean(prepared), targetDir: batch.work, staging: true });
    stageAndroidAbi(batch, { abi, ...output });
  }
  return finishAndroidOutput(batch);
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const { values, positionals } = parseArgs({ allowPositionals: true, options: {
      target: { type: 'string' }, zig: { type: 'string', default: 'zig' }, 'output-root': { type: 'string' },
      native002: { type: 'boolean' }, 'prepared-vt-map': { type: 'string' }, patch: { type: 'string' },
    } });
    if (positionals.length !== 1) throw new Error('Expected plan, build, or build-android.');
    if (positionals[0] === 'build-android' && !values.target) {
      if (values['prepared-vt-map'] && !values.native002) throw new Error('Prepared VT map is explicit Native002 only.');
      const prepared = values['prepared-vt-map'] ? JSON.parse(readFileSync(values['prepared-vt-map'], 'utf8')) : undefined;
      if (prepared && (typeof prepared !== 'object' || Array.isArray(prepared) || Object.entries(prepared).some(([abi, p]) => !androidTargets[abi] || typeof p !== 'string'))) throw new Error('Invalid prepared VT map.');
      console.log(buildAndroid(values.zig, values['output-root'] ? resolve(values['output-root']) : undefined,
        { native002: values.native002 === true, prepared, patch: values.patch }));
    } else {
      if (!values.target || !['plan', 'build'].includes(positionals[0])) {
        throw new Error('Usage: build-mobile.mjs plan|build --target <triple>, or build-android [--output-root <new project tmp root>] [--zig <cached Zig 0.17.0>]');
      }
      const recipe = plan(values.target); console.log(JSON.stringify(recipe, null, 2));
      if (positionals[0] === 'build') build(recipe, values.zig);
    }
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
