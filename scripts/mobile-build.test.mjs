import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { androidBuildToolsDirectory, androidBuildToolsVersion, mobileVersion, signingEnvironment, verifyApkSigner } from './mobile-build.mjs';
import { aptPolicy, configureUbuntuApt, rewriteSources, validateMirror } from '../.github/scripts/setup-ubuntu-apt.mjs';

const signing = {
  SHELLFOX_ANDROID_KEYSTORE_BASE64: 'YQ==', SHELLFOX_ANDROID_STORE_PASSWORD: 'test-store',
  SHELLFOX_ANDROID_KEY_ALIAS: 'release', SHELLFOX_ANDROID_KEY_PASSWORD: 'test-key',
  SHELLFOX_ANDROID_CERT_SHA256: 'ab'.repeat(32),
};
describe('mobile release contract', () => {
  it('selects pinned build-tools even when a newer SDK is installed and fails closed if the pin is missing', () => {
    mkdirSync('tmp', { recursive: true });
    const sdk = mkdtempSync(path.resolve('tmp/mobile-sdk-test-'));
    try {
      for (const version of [androidBuildToolsVersion, '37.0.0']) {
        const directory = path.join(sdk, 'build-tools', version);
        mkdirSync(path.join(directory, 'lib'), { recursive: true });
        writeFileSync(path.join(directory, 'lib/apksigner.jar'), 'fixture');
        writeFileSync(path.join(directory, 'aapt' + (process.platform === 'win32' ? '.exe' : '')), 'fixture');
      }
      const pinned = path.join(sdk, 'build-tools', androidBuildToolsVersion);
      assert.equal(androidBuildToolsDirectory(sdk), pinned);
      rmSync(pinned, { recursive: true, force: true });
      assert.throws(() => androidBuildToolsDirectory(sdk), /build-tools 36\.0\.0 are required/);
    } finally { rmSync(sdk, { recursive: true, force: true }); }
  });
  it('keeps the verifier SDK pin aligned with Gradle and the workflow installation', () => {
    const gradle = readFileSync('mobile/android/build.gradle', 'utf8');
    const workflow = readFileSync('.github/workflows/release.yml', 'utf8');
    assert.ok(gradle.includes(`buildToolsVersion = "${androidBuildToolsVersion}"`));
    assert.ok(workflow.includes(`'build-tools;${androidBuildToolsVersion}'`));
  });
  it('retains the exact single-signer certificate guard and diagnoses unrecognized SDK output without trusting it', () => {
    const certificate = 'ab'.repeat(32), foreign = 'cd'.repeat(32);
    const single = `Number of signers: 1\nSigner #1 certificate SHA-256 digest: ${certificate}\n`;
    assert.doesNotThrow(() => verifyApkSigner(single, certificate.toUpperCase()));
    assert.doesNotThrow(() => verifyApkSigner(single));
    assert.throws(() => verifyApkSigner(single, foreign), /parsed signers=1/);
    assert.throws(() => verifyApkSigner(single + `Signer #2 certificate SHA-256 digest: ${certificate}\n`, certificate), /parsed signers=2/);
    const newerSDK = `Number of signers: 1\nV2 Signer: certificate SHA-256 digest: ${certificate}\n`;
    assert.throws(() => verifyApkSigner(newerSDK, certificate), new RegExp(`parsed signers=0; reported SHA-256=${certificate}`));
    assert.throws(() => verifyApkSigner('', certificate), /parsed signers=0; reported SHA-256=none/);
  });
  it('maps stable versions to monotonically increasing Android version codes', () => {
    assert.deepEqual(mobileVersion('0.8.0'), { name: '0.8.0', code: 8000 });
    assert.ok(mobileVersion('1.0.0').code > mobileVersion('0.999.999').code);
    assert.ok(mobileVersion('0.10.0').code > mobileVersion('0.9.999').code);
    for (const value of ['0.0.0', 'v0.8.0', '0.8.0-beta', '0.8.0+1', '00.8.0', '1000.0.0', '0.1000.0', '0.0.1000', null]) {
      assert.throws(() => mobileVersion(value));
    }
  });
  it('fails closed if any signing secret is absent', () => {
    for (const key of Object.keys(signing)) {
      const incomplete = { ...signing }; delete incomplete[key];
      assert.throws(() => signingEnvironment(incomplete, '/private/release.keystore'), /Release requires/);
    }
    assert.throws(() => signingEnvironment({ ...signing, SHELLFOX_ANDROID_CERT_SHA256: 'wrong' }, 'key'));
    assert.throws(() => signingEnvironment({ ...signing, SHELLFOX_ANDROID_KEY_ALIAS: 'androiddebugkey' }, 'key'));
    const env = signingEnvironment(signing, '/private/release.keystore');
    assert.equal(env.ORG_GRADLE_PROJECT_SHELLFOX_UPLOAD_STORE_FILE, '/private/release.keystore');
    assert.equal(env.ORG_GRADLE_PROJECT_SHELLFOX_UPLOAD_KEY_PASSWORD, 'test-key');
    assert.equal(Object.keys(env).length, 4);
  });
  it('keeps desktop and headless coverage and gates publication on the mobile job', () => {
    const workflow = readFileSync('.github/workflows/release.yml', 'utf8');
    assert.match(workflow, /needs: \[package, headless, mobile\]/);
    assert.match(workflow, /dist\/\*\*\/\*\.apk/);
    assert.match(workflow, /shellfox-mobile-\*-android\.apk/);
    // The action defaults include obsolete SDK "tools", removed upstream.
    assert.match(workflow, /uses: android-actions\/setup-android@v3\s+with:\s+packages: ['"]platform-tools['"]/);
    assert.doesNotMatch(workflow, /mobile-build\.mjs --test/);
    for (const key of Object.keys(signing)) assert.ok(workflow.includes(`secrets.${key}`));
    for (const target of ['linux', 'win32', 'darwin']) {
      assert.ok(workflow.includes(`platform: ${target}`));
      assert.ok(readFileSync('.github/workflows/headless-packages.yml', 'utf8').includes(`platform: ${target}`));
    }
  });
  it('keeps the native terminal fixture isolated from distributable apps', () => {
    const build = readFileSync('scripts/mobile-build.mjs', 'utf8');
    const gradle = readFileSync('mobile/android/app/build.gradle', 'utf8');
    const workflow = readFileSync('.github/workflows/release.yml', 'utf8');
    assert.match(build, /delete env\.ENTRY_FILE/);
    assert.match(gradle, /terminalTest \{[\s\S]*?applicationIdSuffix ["']\.terminaltest["']/);
    assert.match(workflow, /:app:assembleTerminalTest/);
    assert.match(workflow, /mobile-terminal-native-e2e\.mjs --serial emulator-5554/);
  });
  it('keeps iOS source versions aligned with the distributable apps', () => {
    const version = JSON.parse(readFileSync('package.json', 'utf8')).version;
    assert.equal(JSON.parse(readFileSync('mobile/package.json', 'utf8')).version, version);
    const expected = mobileVersion(version);
    const project = readFileSync('mobile/ios/ShellfoxMobile.xcodeproj/project.pbxproj', 'utf8');
    const names = [...project.matchAll(/MARKETING_VERSION = ([^;]+);/g)].map(match => match[1]);
    const codes = [...project.matchAll(/CURRENT_PROJECT_VERSION = (\d+);/g)].map(match => Number(match[1]));
    assert.deepEqual(names, [expected.name, expected.name]);
    assert.deepEqual(codes, [expected.code, expected.code]);
  });
  it('installs browser prerequisites before browser-backed unit suites', () => {
    const workflow = readFileSync('.github/workflows/release.yml', 'utf8');
    const mobile = workflow.slice(workflow.indexOf('  mobile:'), workflow.indexOf('  package:'));
    assert.ok(mobile.indexOf('- name: Install browser test prerequisites') >= 0);
    assert.ok(mobile.indexOf('- name: Install browser test prerequisites') < mobile.indexOf('- name: Mobile updater and build contracts'));
    const desktop = workflow.slice(workflow.indexOf('  package:'));
    assert.ok(desktop.indexOf('- name: Browser unit-test prerequisites') >= 0);
    assert.ok(desktop.indexOf('- name: Browser unit-test prerequisites') < desktop.indexOf('- name: Unit tests'));
    const embedded = readFileSync('.github/workflows/embedded-packages.yml', 'utf8');
    assert.ok(embedded.indexOf('- name: Browser unit-test prerequisites') >= 0);
    assert.ok(embedded.indexOf('- name: Browser unit-test prerequisites') < embedded.indexOf('run: pnpm test:unit'));
    assert.match(embedded, /pnpm test:unit && pnpm test:packaging\s+timeout-minutes: 10/);
  });
  it('gives cold browser dependencies time and shares archive setup with other apt consumers', () => {
    const install = readFileSync('.github/scripts/install-browser-tests.sh', 'utf8');
    assert.match(install, /setup-ubuntu-apt\.sh/);
    assert.ok(install.indexOf('setup-ubuntu-apt.sh') < install.indexOf('pnpm exec playwright'));
    assert.match(install, /pnpm exec playwright install --with-deps chromium/);
    for (const workflow of ['release.yml', 'embedded-packages.yml']) {
      const source = readFileSync('.github/workflows/' + workflow, 'utf8');
      assert.match(source, /run: bash \.github\/scripts\/install-browser-tests\.sh\s+timeout-minutes: 15/);
    }
  });
  it('configures Ubuntu archives before any Linux apt consumers in every packaging job', () => {
    const release = readFileSync('.github/workflows/release.yml', 'utf8');
    const workflows = ['release.yml', 'embedded-packages.yml', 'headless-packages.yml'];
    for (const name of workflows) {
      const workflow = readFileSync('.github/workflows/' + name, 'utf8');
      for (const variable of ['SHELLFOX_UBUNTU_MIRROR', 'SHELLFOX_UBUNTU_PORTS_MIRROR']) {
        assert.ok(workflow.includes(variable + ': ${{ vars.' + variable + ' }}'));
      }
    }
    const jobs = [
      release.slice(release.indexOf('  mobile:'), release.indexOf('  package:')),
      release.slice(release.indexOf('  package:'), release.indexOf('  release:')),
      ...workflows.slice(1).map(name => readFileSync('.github/workflows/' + name, 'utf8')),
    ];
    for (const job of jobs) {
      const setup = job.indexOf('run: bash .github/scripts/setup-ubuntu-apt.sh');
      assert.ok(setup >= 0);
      for (const consumer of ['sudo apt-get', 'run: bash .github/scripts/install-browser-tests.sh']) {
        const index = job.indexOf(consumer);
        if (index >= 0) assert.ok(setup < index);
      }
    }
    const setup = readFileSync('.github/scripts/setup-ubuntu-apt.sh', 'utf8');
    assert.match(setup, /GITHUB_ACTIONS:-.*true/);
    assert.match(setup, /RUNNER_OS:-.*Linux/);
    assert.match(setup, /\$ID.*ubuntu/);
    assert.match(setup, /dpkg --print-architecture/);
    assert.match(setup, /amd64\) mirror="\$\{SHELLFOX_UBUNTU_MIRROR:-https:\/\/mirrors\.edge\.kernel\.org\/ubuntu\}"/);
    assert.match(setup, /arm64\) mirror="\$\{SHELLFOX_UBUNTU_PORTS_MIRROR:-https:\/\/mirrors\.mit\.edu\/ubuntu-ports\}"/);
  });
  it('uses a separate preview build and never a debug-signed release fallback', () => {
    const gradle = readFileSync('mobile/android/app/build.gradle', 'utf8');
    assert.match(gradle, /preview \{[\s\S]*?initWith release[\s\S]*?signingConfig signingConfigs\.debug[\s\S]*?debuggable false/);
    assert.match(gradle, /Release APK requires all SHELLFOX_UPLOAD_\*/);
    assert.match(gradle, /implementation\(files\("\.\.\/\.\.\/\.\.\/tmp\/mobile-native\/android\/shellfox-mobile\.aar"\)\)/);
  });
});

describe('Ubuntu CI archive configuration', () => {
  const mirrors = ['https://mirrors.edge.kernel.org/ubuntu', 'https://mirrors.mit.edu/ubuntu-ports'];
  const key = 'Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg';
  const stanza = uri => `Types: deb deb-src\nURIs: ${uri}\nSuites: noble noble-updates noble-backports noble-security\nComponents: main restricted universe multiverse\n${key}\n`;

  it('replaces GitHub mirror lists and direct Ubuntu sources without changing suites or signing keys', () => {
    const uris = [
      'mirror+file:/etc/apt/apt-mirrors.txt',
      'mirror+file:/etc/apt/apt-mirrors-security.txt',
      'http://azure.archive.ubuntu.com/ubuntu/',
      'https://azure.archive.ubuntu.com/ubuntu',
      'http://archive.ubuntu.com/ubuntu',
      'https://us.archive.ubuntu.com/ubuntu',
      'http://security.ubuntu.com/ubuntu',
      'http://ports.ubuntu.com/ubuntu-ports/',
      'https://ports.ubuntu.com/ubuntu-ports',
    ];
    for (const mirror of mirrors) {
      for (const uri of uris) {
        assert.equal(rewriteSources(stanza(uri), '.sources', mirror), stanza(mirror));
      }
      const multi = stanza(uris[0] + '\n ' + uris[1]);
      assert.equal(rewriteSources(multi, '.sources', mirror), stanza(mirror + '\n ' + mirror));
    }
  });

  it('supports legacy lists and leaves third-party archives, comments and other fields alone', () => {
    const uri = 'http://azure.archive.ubuntu.com/ubuntu';
    const mirror = mirrors[0];
    const thirdParty = '# ' + uri + '\n' + stanza('https://packages.microsoft.com/repos/code');
    assert.equal(rewriteSources(thirdParty, '.sources', mirror), thirdParty);
    const legacy = `deb [arch=amd64 signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg] ${uri} noble main\ndeb-src ${uri} noble-updates universe\n`;
    assert.equal(rewriteSources(legacy, '.list', mirror), legacy.replaceAll(uri, mirror));
    const untouched = '# deb ' + uri + ' noble main\ndeb https://ppa.launchpadcontent.net/test/ubuntu noble main\n' +
      'deb https://archive.ubuntu.com/ubuntu-other noble main\n';
    assert.equal(rewriteSources(untouched, '.list', mirror), untouched);
    assert.equal(rewriteSources(`Description: ${uri}\n` + stanza(mirror), '.sources', mirror), `Description: ${uri}\n` + stanza(mirror));
  });

  it('rejects insecure mirrors and apt syntax injection before changing any files', () => {
    for (const value of [undefined, '', 'http://mirror.example/ubuntu', 'file:/archive',
      'https://user:password@mirror.example/ubuntu', 'https://mirror.example/ubuntu?x=1',
      'https://mirror.example/ubuntu#fragment', 'https://mirror.example/ubuntu\nTrusted: yes',
      'https://mirror.example/ubuntu other', 'https://mirror.example\\ubuntu']) {
      assert.throws(() => validateMirror(value));
    }
    assert.equal(validateMirror('https://mirror.example/ubuntu/'), 'https://mirror.example/ubuntu');
  });

  it('updates all apt source files idempotently and makes failed index downloads fatal', () => {
    mkdirSync('tmp', { recursive: true });
    const directory = mkdtempSync(path.resolve('tmp/ubuntu-apt-test-'));
    try {
      mkdirSync(path.join(directory, 'sources.list.d'));
      mkdirSync(path.join(directory, 'apt.conf.d'));
      const main = path.join(directory, 'sources.list');
      const ubuntu = path.join(directory, 'sources.list.d/ubuntu.sources');
      const legacy = path.join(directory, 'sources.list.d/ubuntu-security.list');
      const vendor = path.join(directory, 'sources.list.d/vendor.sources');
      const backup = path.join(directory, 'sources.list.d/ubuntu.sources.bak');
      const content = stanza('mirror+file:/etc/apt/apt-mirrors.txt');
      const vendorContent = stanza('https://packages.microsoft.com/repos/code');
      writeFileSync(main, 'deb http://archive.ubuntu.com/ubuntu noble main\n');
      writeFileSync(ubuntu, content);
      writeFileSync(legacy, 'deb http://security.ubuntu.com/ubuntu noble-security main\n');
      writeFileSync(vendor, vendorContent);
      writeFileSync(backup, content);
      assert.throws(() => configureUbuntuApt(directory, 'http://invalid.example/ubuntu'));
      assert.equal(readFileSync(ubuntu, 'utf8'), content);
      assert.equal(configureUbuntuApt(directory, mirrors[1]), 3);
      assert.equal(readFileSync(ubuntu, 'utf8'), stanza(mirrors[1]));
      assert.equal(readFileSync(vendor, 'utf8'), vendorContent);
      assert.equal(readFileSync(backup, 'utf8'), content);
      assert.equal(configureUbuntuApt(directory, mirrors[1]), 0);
      const policy = readFileSync(path.join(directory, 'apt.conf.d/80shellfox-archives'), 'utf8');
      assert.equal(policy, aptPolicy);
      assert.match(policy, /APT::Update::Error-Mode "any"/);
      assert.match(policy, /Acquire::Retries "3"/);
      assert.doesNotMatch(policy, /AllowInsecure|AllowUnauthenticated|Verify-Peer|Trusted/i);
    } finally { rmSync(directory, { recursive: true, force: true }); }
  });
});
