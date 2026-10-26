import assert from 'node:assert/strict';
import test from 'node:test';
import { bindingArgs, main, mobileVersion } from './mobile-native.mjs';

test('Android targets match the app universal 64-bit APK and minimum SDK', () => {
  const args = bindingArgs('android', '/repo/tmp/mobile-native/android/shellfox-mobile.aar');
  assert.ok(args.includes('-target=android/arm64,android/amd64'));
  assert.ok(args.includes('-androidapi=24'));
  assert.ok(args.includes('-trimpath'));
  assert.equal(args.at(-1), '.');
  assert.match(mobileVersion, /^v0\.0\.0-\d{14}-[a-f0-9]{12}$/);
});

test('iOS binds both device and simulator without changing Mobile framework name', () => {
  const args = bindingArgs('ios', '/repo/tmp/mobile-native/ios/.staging/Mobile.xcframework');
  assert.ok(args.includes('-target=ios,iossimulator'));
  assert.ok(args.includes('-iosversion=15.1'));
  assert.equal(args[args.indexOf('-o') + 1], '/repo/tmp/mobile-native/ios/.staging/Mobile.xcframework');
});

test('unknown targets and CLI commands fail before executing external tools', () => {
  assert.throws(() => bindingArgs('linux', 'wrong'), /android or ios/);
  assert.throws(() => main(['unknown']), /Usage/);
  assert.throws(() => main(['build', 'linux']), /android or ios/);
  assert.throws(() => main(['test', 'android', 'surprise']), /Usage/);
  if (process.platform !== 'darwin') assert.throws(() => main(['build', 'ios']), /macOS/);
});
