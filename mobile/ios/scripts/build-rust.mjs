import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { withPinnedZig } from '../../../scripts/rust-package-prereqs.mjs';

export const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
export const slices = Object.freeze([
  { target: 'aarch64-apple-ios', sdk: 'iphoneos', zig: 'aarch64-ios.15.1' },
  { target: 'aarch64-apple-ios-sim', sdk: 'iphonesimulator', zig: 'aarch64-ios.15.1-simulator' },
  { target: 'x86_64-apple-ios', sdk: 'iphonesimulator', zig: 'x86_64-ios.15.1-simulator' },
]);
export function minimumIOS() {
  const helper = readFileSync(join(root, 'node_modules/react-native/scripts/cocoapods/helpers.rb'), 'utf8');
  const minimum = /def self\.min_ios_version_supported\s+return '([^']+)'/.exec(helper)?.[1];
  if (minimum !== '15.1') throw new Error(`Review iOS build recipes for installed RN minimum ${minimum}`);
  return minimum;
}
export function run(command, args, env = process.env, capture = false) {
  const result = spawnSync(command, args, { cwd: root, env, timeout: 30 * 60_000,
    encoding: 'utf8', stdio: capture ? ['ignore', 'pipe', 'pipe'] : 'inherit' });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} failed (${result.status}): ${capture ? result.stderr : 'see output'}`);
  return capture === 'both' ? `${result.stdout}\n${result.stderr}` : capture ? result.stdout.trim() : '';
}
export function nativeLinkSettings(output) {
  const line = /native-static-libs:\s*([^\r\n]+)/.exec(output)?.[1];
  if (!line) throw new Error('rustc did not report native-static-libs; review the saved build log');
  const tokens = line.match(/"[^"]*"|'[^']*'|\S+/g) ?? [];
  const libraries = new Set(), frameworks = new Set();
  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i].replace(/^['"]|['"]$/g, '');
    if (/^-l[A-Za-z0-9_.+-]+$/.test(token)) libraries.add(token.slice(2));
    else if (token === '-framework' && /^[A-Za-z0-9_.+-]+$/.test(tokens[i + 1] ?? '')) frameworks.add(tokens[++i]);
    else throw new Error(`Review unsupported rustc native linker option: ${token}`);
  }
  return { libraries: [...libraries].sort(), frameworks: [...frameworks].sort() };
}
export function runtimePodspec(link = { libraries: [], frameworks: [] }) {
  return `Pod::Spec.new do |s|
  s.name = 'ShellfoxRustRuntime'
  s.version = '0.1.0'
  s.summary = 'Shellfox Rust remote client and Ghostty static runtime'
  s.homepage = 'https://github.com/oira666/shellfox'
  s.license = { :type => 'MIT', :file => 'notices/shellfox-LICENSE' }
  s.author = 'Shellfox'
  s.source = { :git => 'https://github.com/oira666/shellfox.git', :tag => s.version.to_s }
  s.platform = :ios, '15.1'
  s.vendored_frameworks = 'ShellfoxMobileRust.xcframework'
  s.preserve_paths = 'notices/**/*'
  s.resources = 'notices'
${link.libraries.length ? `  s.libraries = ${link.libraries.map(name => `'${name}'`).join(', ')}\n` : ''}${link.frameworks.length ? `  s.frameworks = ${link.frameworks.map(name => `'${name}'`).join(', ')}\n` : ''}end
`;
}
export function build(zig = 'zig') {
  minimumIOS();
  if (process.platform !== 'darwin') throw new Error('iOS native compilation requires macOS, Xcode and CocoaPods. No native build ran.');
  run('xcodebuild', ['-version']);
  const work = join(root, 'tmp/ios-rust-build');
  const destination = join(root, 'tmp/mobile-rust/ios');
  const framework = join(destination, 'ShellfoxMobileRust.xcframework');
  if (existsSync(framework)) throw new Error(`Remove the old owned build explicitly before rebuilding: ${framework}`);
  mkdirSync(work, { recursive: true }); mkdirSync(destination, { recursive: true });
  const cargo = resolve(root, process.env.CARGO_TARGET_DIR || join(work, 'cargo'));
  const nativeLibraries = new Set(), nativeFrameworks = new Set();
  for (const slice of slices) {
    const sdkroot = run('xcrun', ['--sdk', slice.sdk, '--show-sdk-path'], process.env, true);
    const clang = run('xcrun', ['--sdk', slice.sdk, '--find', 'clang'], process.env, true);
    const env = { ...process.env, SDKROOT: sdkroot, IPHONEOS_DEPLOYMENT_TARGET: minimumIOS(),
      CARGO_TARGET_DIR: cargo, GHOSTTY_VT_LINK_KIND: 'static',
      [`CC_${slice.target.replaceAll('-', '_')}`]: clang };
    const prefix = join(work, `ghostty-${slice.target}`);
    if (!existsSync(prefix)) run(process.execPath, ['crates/ghostty-vt-sys/build-support/prepare.mjs', 'build',
      '--target', slice.target, '--prefix', prefix, '--zig-target', slice.zig, '--zig', zig], env);
    const manifest = JSON.parse(readFileSync(join(prefix, 'ghostty-vt-build.json'), 'utf8'));
    if (manifest.rustTarget !== slice.target || manifest.zigTarget !== slice.zig || manifest.version !== '1.0.1')
      throw new Error(`Wrong prepared Ghostty target/deployment: ${prefix}`);
    const log = run('cargo', ['rustc', '--locked', '--release', '--lib', '-p', 'shellfox-mobile', '--target', slice.target,
      '--', '--print', 'native-static-libs'], { ...env, GHOSTTY_VT_DIR: prefix }, 'both');
    writeFileSync(join(work, `${slice.target}-link.txt`), log);
    process.stdout.write(log);
    const settings = nativeLinkSettings(log);
    settings.libraries.forEach(name => nativeLibraries.add(name));
    settings.frameworks.forEach(name => nativeFrameworks.add(name));
  }
  const simulator = join(work, 'simulator'); mkdirSync(simulator, { recursive: true });
  const fat = join(simulator, 'libshellfox_mobile.a');
  run('xcrun', ['lipo', '-create', ...slices.filter(s => s.sdk === 'iphonesimulator')
    .map(s => join(cargo, s.target, 'release/libshellfox_mobile.a')), '-output', fat]);
  let complete = false;
  try {
    const headers = join(root, 'crates/shellfox-mobile/include');
    run('xcodebuild', ['-create-xcframework', '-library', join(cargo, slices[0].target, 'release/libshellfox_mobile.a'),
      '-headers', headers, '-library', fat, '-headers', headers, '-output', framework]);
    const notices = join(destination, 'notices'); mkdirSync(notices, { recursive: true });
    cpSync(join(root, 'LICENSE'), join(notices, 'shellfox-LICENSE'));
    cpSync(join(work, `ghostty-${slices[0].target}`, 'share/ghostty-vt-notices'), join(notices, 'ghostty'), { recursive: true });
    writeFileSync(join(destination, 'ShellfoxRustRuntime.podspec'), runtimePodspec({ libraries: [...nativeLibraries].sort(), frameworks: [...nativeFrameworks].sort() }));
    writeFileSync(join(destination, 'build.json'), JSON.stringify({ minimumIOS: minimumIOS(), slices }, null, 2) + '\n');
    complete = true;
  } finally {
    if (!complete) rmSync(framework, { recursive: true, force: true });
  }
  console.log(`Built unsigned runtime at ${framework}. Run pod install and the unsigned simulator build next.`);
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (process.argv.includes('--plan')) console.log(JSON.stringify({ minimumIOS: minimumIOS(), slices }, null, 2));
    else if (process.argv.includes('--fetch-zig') && process.platform === 'darwin') await withPinnedZig(build);
    else build();
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
