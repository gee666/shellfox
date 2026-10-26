import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { mobileVersion, signingEnvironment } from './mobile-build.mjs';

const signing = {
  SHELLFOX_ANDROID_KEYSTORE_BASE64: 'YQ==', SHELLFOX_ANDROID_STORE_PASSWORD: 'test-store',
  SHELLFOX_ANDROID_KEY_ALIAS: 'release', SHELLFOX_ANDROID_KEY_PASSWORD: 'test-key',
  SHELLFOX_ANDROID_CERT_SHA256: 'ab'.repeat(32),
};
describe('mobile release contract', () => {
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
    assert.doesNotMatch(workflow, /mobile-build\.mjs --test/);
    for (const key of Object.keys(signing)) assert.ok(workflow.includes(`secrets.${key}`));
    for (const target of ['linux', 'win32', 'darwin']) {
      assert.ok(workflow.includes(`platform: ${target}`));
      assert.ok(readFileSync('.github/workflows/headless-packages.yml', 'utf8').includes(`platform: ${target}`));
    }
  });
  it('uses a separate preview build and never a debug-signed release fallback', () => {
    const gradle = readFileSync('mobile/android/app/build.gradle', 'utf8');
    assert.match(gradle, /preview \{[\s\S]*?initWith release[\s\S]*?signingConfig signingConfigs\.debug[\s\S]*?debuggable false/);
    assert.match(gradle, /Release APK requires all SHELLFOX_UPLOAD_\*/);
    assert.match(gradle, /implementation\(files\("\.\.\/\.\.\/\.\.\/tmp\/mobile-native\/android\/shellfox-mobile\.aar"\)\)/);
  });
});
