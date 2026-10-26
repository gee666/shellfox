import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync, copyFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export function mobileVersion(version) {
  if (typeof version !== 'string' || !/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version)) throw new Error('Expected a stable mobile version.');
  const parts = version.split('.').map(Number);
  if (parts.some(value => value > 999)) throw new Error('Mobile version components must be between 0 and 999.');
  const code = parts[0] * 1_000_000 + parts[1] * 1000 + parts[2];
  if (!code) throw new Error('Mobile versionCode must be positive.');
  return { name: version, code };
}

export function signingEnvironment(environment, keystore) {
  const names = ['KEYSTORE_BASE64', 'STORE_PASSWORD', 'KEY_ALIAS', 'KEY_PASSWORD', 'CERT_SHA256'];
  for (const name of names) if (!environment[`SHELLFOX_ANDROID_${name}`]) throw new Error(`Release requires SHELLFOX_ANDROID_${name}.`);
  if (!/^[a-fA-F0-9]{64}$/.test(environment.SHELLFOX_ANDROID_CERT_SHA256)) throw new Error('Signing certificate SHA-256 must be 64 hexadecimal characters.');
  if (environment.SHELLFOX_ANDROID_KEY_ALIAS === 'androiddebugkey') throw new Error('Debug signing is forbidden for releases.');
  return {
    ORG_GRADLE_PROJECT_SHELLFOX_UPLOAD_STORE_FILE: keystore,
    ORG_GRADLE_PROJECT_SHELLFOX_UPLOAD_STORE_PASSWORD: environment.SHELLFOX_ANDROID_STORE_PASSWORD,
    ORG_GRADLE_PROJECT_SHELLFOX_UPLOAD_KEY_ALIAS: environment.SHELLFOX_ANDROID_KEY_ALIAS,
    ORG_GRADLE_PROJECT_SHELLFOX_UPLOAD_KEY_PASSWORD: environment.SHELLFOX_ANDROID_KEY_PASSWORD,
  };
}

export function main(args = process.argv.slice(2)) {
  if (args.some(arg => arg !== '--test') || args.length > 1) throw new Error('Usage: node scripts/mobile-build.mjs [--test]');
  const test = args.includes('--test');
  if (test && process.env.GITHUB_REF?.startsWith('refs/tags/')) throw new Error('Tag releases cannot use test signing.');
  const version = mobileVersion(JSON.parse(readFileSync(path.join(root, 'package.json'), 'utf8')).version);
  if (process.env.GITHUB_REF?.startsWith('refs/tags/') && process.env.GITHUB_REF !== `refs/tags/v${version.name}`) throw new Error('Tag must match root package.json version.');
  const scratch = path.join(root, 'tmp');
  const output = path.join(scratch, 'mobile-artifacts');
  mkdirSync(output, { recursive: true });
  const name = `shellfox-mobile-${version.name}-android${test ? '-test' : ''}.apk`;
  const destination = path.join(output, name);
  rmSync(destination, { force: true });
  rmSync(destination + '.sha256', { force: true });
  const env = { ...process.env, TMP: scratch, TEMP: scratch, TMPDIR: scratch,
    GRADLE_USER_HOME: path.join(scratch, 'gradle-home'), npm_config_cache: path.join(scratch, 'npm-cache') };
  const signingFile = path.join(scratch, 'mobile-release.keystore');
  if (!test) Object.assign(env, signingEnvironment(env, signingFile));
  // Do not let ambient Gradle properties change architectures or inject a debug signing key.
  env.ORG_GRADLE_PROJECT_reactNativeArchitectures = 'arm64-v8a,x86_64';
  const sdk = env.ANDROID_HOME || env.ANDROID_SDK_ROOT;
  if (!sdk) throw new Error('Set ANDROID_HOME to an installed Android SDK.');
  const javaBin = name => env.JAVA_HOME ? path.join(env.JAVA_HOME, 'bin', name + (process.platform === 'win32' ? '.exe' : '')) : name;
  function run(command, arguments_, cwd = root, capture = false, timeout = 1_800_000) {
    const result = spawnSync(command, arguments_, { cwd, env, timeout, windowsHide: true,
      stdio: capture ? 'pipe' : 'inherit', maxBuffer: 16 * 1024 * 1024 });
    if (result.error || result.status !== 0) throw new Error(`Mobile build command failed: ${path.basename(command)} ${arguments_[0] ?? ''}. ${result.error?.message ?? ''}`);
    return result.stdout;
  }
  try {
    if (!test) {
      const encoded = env.SHELLFOX_ANDROID_KEYSTORE_BASE64;
      if (!/^[A-Za-z0-9+/]+={0,2}$/.test(encoded) || encoded.length % 4 !== 0) throw new Error('Invalid base64 release keystore.');
      writeFileSync(signingFile, Buffer.from(encoded, 'base64'), { mode: 0o600, flag: 'wx' });
      const certificate = run(javaBin('keytool'), ['-exportcert', '-keystore', signingFile,
        '-alias', env.SHELLFOX_ANDROID_KEY_ALIAS, '-storepass:env', 'SHELLFOX_ANDROID_STORE_PASSWORD'], root, true, 30_000);
      const actual = createHash('sha256').update(certificate).digest('hex');
      if (actual !== env.SHELLFOX_ANDROID_CERT_SHA256.toLowerCase()) throw new Error('Release keystore does not match the pinned signing certificate.');
      const debugCert = run(javaBin('keytool'), ['-exportcert', '-keystore', path.join(root, 'mobile/android/app/debug.keystore'),
        '-alias', 'androiddebugkey', '-storepass', 'android'], root, true, 30_000);
      if (actual === createHash('sha256').update(debugCert).digest('hex')) throw new Error('Debug certificate is forbidden for releases.');
    }
    run(process.execPath, ['scripts/mobile-terminal.mjs']);
    const gomobile = env.SHELLFOX_GOMOBILE || path.join(scratch, 'mobile-native/go/bin/gomobile' + (process.platform === 'win32' ? '.exe' : ''));
    if (!existsSync(gomobile)) run(process.execPath, ['scripts/mobile-native.mjs', 'setup']);
    run(process.execPath, ['scripts/mobile-native.mjs', 'build', 'android']);
    const android = path.join(root, 'mobile/android');
    run(javaBin('java'), ['-classpath', path.join(android, 'gradle/wrapper/gradle-wrapper.jar'),
      'org.gradle.wrapper.GradleWrapperMain', '--no-daemon', '--console=plain',
      '-PreactNativeArchitectures=arm64-v8a,x86_64', test ? ':app:assemblePreview' : ':app:assembleRelease'], android);
    const variant = test ? 'preview' : 'release';
    const apk = path.join(android, `app/build/outputs/apk/${variant}/app-${variant}.apk`);
    if (!existsSync(apk)) throw new Error('Universal APK was not produced.');
    const tools = readdirSync(path.join(sdk, 'build-tools')).filter(name => /^\d+\.\d+\.\d+$/.test(name))
      .sort((a, b) => b.localeCompare(a, 'en', { numeric: true }));
    if (!tools[0]) throw new Error('Android build-tools are required.');
    const toolDirectory = path.join(sdk, 'build-tools', tools[0]);
    const verified = run(javaBin('java'), ['-jar', path.join(toolDirectory, 'lib/apksigner.jar'), 'verify', '--verbose', '--print-certs', apk], root, true, 60_000).toString('utf8');
    const certs = [...verified.matchAll(/^Signer #\d+ certificate SHA-256 digest: ([a-fA-F0-9]{64})\r?$/gm)];
    if (certs.length !== 1 || (!test && certs[0][1].toLowerCase() !== env.SHELLFOX_ANDROID_CERT_SHA256.toLowerCase())) throw new Error('APK signing certificate verification failed.');
    const badging = run(path.join(toolDirectory, 'aapt' + (process.platform === 'win32' ? '.exe' : '')), ['dump', 'badging', apk], root, true, 60_000).toString('utf8');
    if (!badging.includes(`package: name='com.shellfox.mobile' versionCode='${version.code}' versionName='${version.name}'`) ||
        !/^native-code: 'arm64-v8a' 'x86_64'\r?$/m.test(badging) || /^application-debuggable/m.test(badging)) throw new Error('APK package, version, architecture or embedded-build validation failed.');
    copyFileSync(apk, destination);
    const hash = createHash('sha256').update(readFileSync(destination)).digest('hex');
    writeFileSync(destination + '.sha256', `${hash}  ${name}\n`);
    console.log(`[mobile-build] ${destination}`);
  } catch (error) {
    rmSync(destination, { force: true }); rmSync(destination + '.sha256', { force: true });
    throw error;
  } finally { if (!test) rmSync(signingFile, { force: true }); }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
