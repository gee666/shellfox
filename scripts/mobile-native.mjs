import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const scratch = path.join(root, 'tmp');
const source = path.join(root, 'native/mobile');
const output = path.join(scratch, 'mobile-native');
export const mobileVersion = 'v0.0.0-20261011034528-197770444867';
export const goVersion = '1.27.3';
const exe = process.platform === 'win32' ? '.exe' : '';
const cachedGo = path.join(scratch, 'go-toolchain/go/bin/go' + exe);
const go = process.env.SHELLFOX_GO || (existsSync(cachedGo) ? cachedGo : 'go');
const bin = path.join(output, 'go/bin');
const gomobile = process.env.SHELLFOX_GOMOBILE || path.join(bin, 'gomobile' + exe);
const directories = [output, bin, 'go-cache', 'go-mod', 'go-work', 'gradle-home'].map(value => path.isAbsolute(value) ? value : path.join(scratch, value));
for (const directory of directories) mkdirSync(directory, { recursive: true });
const env = { ...process.env, GOTOOLCHAIN: 'local', GOENV: 'off', GOWORK: 'off', GOFLAGS: '',
  GOPATH: path.join(output, 'go'), GOBIN: bin, GOCACHE: path.join(scratch, 'go-cache'),
  GOMODCACHE: path.join(scratch, 'go-mod'), GOTMPDIR: path.join(scratch, 'go-work'),
  TMP: scratch, TEMP: scratch, TMPDIR: scratch, GRADLE_USER_HOME: path.join(scratch, 'gradle-home'),
  PATH: [path.dirname(go), bin, process.env.PATH].join(path.delimiter) };
// Node on Windows treats Path and PATH as the same variable. Keep one spelling.
for (const key of Object.keys(env)) if (key !== 'PATH' && key.toUpperCase() === 'PATH') delete env[key];

function run(command, args, extra = {}, timeout = 600_000) {
  console.log('[mobile-native]', command, ...args);
  const result = spawnSync(command, args, { cwd: source, env: { ...env, ...extra }, stdio: 'inherit', timeout, windowsHide: true });
  if (result.error || result.status !== 0) throw new Error(`Native mobile command failed (${result.status ?? 'spawn'}): ${result.error?.message ?? command}`);
}

export function bindingArgs(target, destination) {
  if (!['android', 'ios'].includes(target)) throw new Error('Target must be android or ios.');
  const flags = target === 'android'
    ? ['-target=android/arm64,android/amd64', '-androidapi=24']
    : ['-target=ios,iossimulator', '-iosversion=15.1'];
  return ['bind', ...flags, '-trimpath', '-ldflags=-s -w', '-o', destination, '.'];
}

function capture(args, extra) {
  const result = spawnSync(go, args, { cwd: source, env: { ...env, ...extra }, encoding: 'utf8', timeout: 60_000, windowsHide: true, maxBuffer: 32 * 1024 * 1024 });
  if (result.error || result.status !== 0) throw new Error(`Cannot collect native dependencies: ${result.error?.message ?? result.stderr}`);
  return result.stdout;
}

function notices(directory, target) {
  const modules = new Map();
  for (const arch of ['arm64', 'amd64']) {
    const rows = capture(['list', '-mod=readonly', '-deps', '-f', '{{with .Module}}{{.Path}}\t{{.Version}}\t{{.Dir}}{{end}}', '.', 'golang.org/x/mobile/bind/seq'], { GOOS: target, GOARCH: arch, CGO_ENABLED: '1' });
    for (const row of rows.split(/\r?\n/)) {
      const [name, version, directory] = row.split('\t');
      // Local shared Shellfox code is first-party, not a third-party notice.
      if (name && version && directory && !name.startsWith('shellfox.local/')) modules.set(name, { version, directory });
    }
  }
  if (!modules.has('tailscale.com') || !modules.has('golang.org/x/mobile')) throw new Error('Embedded dependency graph is incomplete.');
  let text = 'Shellfox embedded Tailscale mobile client\n\n';
  const missing = [];
  for (const [name, { version, directory }] of [...modules].sort(([a], [b]) => a.localeCompare(b))) {
    text += `\n===== ${name} ${version} =====\n`;
    const files = readdirSync(directory, { withFileTypes: true }).filter(entry => entry.isFile() && /^(licen[sc]e|copying|notice|copyright)([._-].*)?$/i.test(entry.name));
    if (!files.length) missing.push(name);
    for (const entry of files) text += `\n--- ${entry.name} ---\n${readFileSync(path.join(directory, entry.name), 'utf8')}\n`;
  }
  const goroot = capture(['env', 'GOROOT']).trim();
  text += `\n===== Go standard library =====\n${readFileSync(path.join(goroot, 'LICENSE'), 'utf8')}\n`;
  const noticePath = path.join(directory, target === 'android' ? 'assets/notices' : 'notices');
  mkdirSync(noticePath, { recursive: true });
  writeFileSync(path.join(noticePath, 'THIRD-PARTY-NOTICES.txt'), text);
  writeFileSync(path.join(noticePath, 'modules.json'), JSON.stringify({ modules: [...modules].map(([name, { version }]) => ({ name, version })), missingModuleRootLicenses: missing }, null, 2) + '\n');
  copyFileSync(path.join(root, 'native/tailscale/TAILSCALE-LICENSE'), path.join(noticePath, 'TAILSCALE-LICENSE'));
  copyFileSync(path.join(source, 'NOTICE-AUDIT.md'), path.join(noticePath, 'NOTICE-AUDIT.md'));
}

function iosPod(directory) {
  const { version } = JSON.parse(readFileSync(path.join(root, 'package.json'), 'utf8'));
  copyFileSync(path.join(root, 'LICENSE'), path.join(directory, 'LICENSE'));
  writeFileSync(path.join(directory, 'ShellfoxEmbeddedNetwork.podspec'), `Pod::Spec.new do |s|
  s.name = 'ShellfoxEmbeddedNetwork'
  s.version = '${version}'
  s.summary = 'Outbound-only embedded Tailscale transport for Shellfox'
  s.homepage = 'https://github.com/gee666/shellfox'
  s.license = { :type => 'MIT', :file => 'LICENSE' }
  s.author = 'Shellfox contributors'
  s.source = { :git => 'https://github.com/gee666/shellfox.git', :tag => 'v${version}' }
  s.ios.deployment_target = '15.1'
  s.vendored_frameworks = 'Mobile.xcframework'
  s.resource_bundles = { 'ShellfoxNetworkNotices' => ['notices/*'] }
end
`);
}

export function main(args = process.argv.slice(2)) {
  const [command = 'test', target = process.platform === 'darwin' ? 'ios' : 'android'] = args;
  if (args.length > 2) throw new Error('Usage: node scripts/mobile-native.mjs setup|test|build [android|ios]');
  if (command === 'setup') {
    for (const tool of ['gomobile', 'gobind']) run(go, ['install', `golang.org/x/mobile/cmd/${tool}@${mobileVersion}`]);
    run(gomobile, ['init']);
  } else if (command === 'test') {
    run(go, ['test', '-mod=readonly', '-count=1', '-timeout=60s', './...', 'shellfox.local/tailscale-helper/nethealth'], { CGO_ENABLED: '0' });
  } else if (command === 'build') {
    if (!['android', 'ios'].includes(target)) throw new Error('Target must be android or ios.');
    if (target === 'ios' && process.platform !== 'darwin') throw new Error('iOS frameworks require macOS with Xcode and iOS SDKs.');
    if (!existsSync(gomobile)) throw new Error('Run node scripts/mobile-native.mjs setup first.');
    const directory = path.join(output, target);
    mkdirSync(directory, { recursive: true });
    const destination = path.join(directory, target === 'android' ? 'shellfox-mobile.aar' : 'Mobile.xcframework');
    // The xcframework basename determines its module name. Keep Mobile intact.
    const staging = path.join(directory, '.staging');
    mkdirSync(staging, { recursive: true });
    const partial = path.join(staging, path.basename(destination));
    // Match RN's NDK by default; ANDROID_NDK_HOME is the explicit override.
    const sdk = env.ANDROID_HOME || env.ANDROID_SDK_ROOT;
    const ndk = sdk && path.join(sdk, 'ndk', '27.1.12297006');
    const buildEnv = { CGO_ENABLED: '1' };
    if (target === 'android' && !env.ANDROID_NDK_HOME && ndk && existsSync(ndk)) buildEnv.ANDROID_NDK_HOME = ndk;
    rmSync(partial, { recursive: true, force: true });
    try {
      run(gomobile, bindingArgs(target, partial), buildEnv, 900_000);
      notices(directory, target);
      if (target === 'ios') iosPod(directory);
      rmSync(destination, { recursive: true, force: true });
      renameSync(partial, destination);
      console.log('[mobile-native] built', destination);
    } finally { rmSync(staging, { recursive: true, force: true }); }
  } else throw new Error('Usage: node scripts/mobile-native.mjs setup|test|build [android|ios]');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
