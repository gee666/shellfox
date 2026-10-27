import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
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

test('Android approval completion has the wake-lock permission, a finite deadline and unconditional cleanup', () => {
  const source = readFileSync(new URL('../mobile/android/app/src/main/java/com/shellfox/mobile/ShellfoxNetworkModule.kt', import.meta.url), 'utf8');
  const manifest = readFileSync(new URL('../mobile/android/app/src/main/AndroidManifest.xml', import.meta.url), 'utf8');
  assert.match(manifest, /<uses-permission android:name="android\.permission\.WAKE_LOCK"\s*\/>/);
  assert.match(source, /path == "\/v1\/pair"/);
  assert.match(source, /newWakeLock\(PowerManager\.PARTIAL_WAKE_LOCK,/);
  assert.match(source, /acquire\(35_000L\)/);
  assert.match(source, /finally \{ if \(wakeLock\?\.isHeld == true\) wakeLock\.release\(\) \}/);
});

test('iOS approval completion uses a finite OS background task with cleanup on both outcomes and expiry', () => {
  const source = readFileSync(new URL('../mobile/ios/ShellfoxMobile/ShellfoxNetwork.mm', import.meta.url), 'utf8');
  assert.match(source, /if \(\[path isEqualToString:@"\/v1\/pair"\]\)/);
  assert.match(source, /beginBackgroundTaskWithName:@"Shellfox approval" expirationHandler:finish/);
  assert.match(source, /endBackgroundTask:task/);
  assert.match(source, /resolve:\^\(id value\) \{ finish\(\); resolve\(value\); \}/);
  assert.match(source, /reject:\^\(NSString \*code, NSString \*message, NSError \*error\) \{ finish\(\); reject\(code, message, error\); \}/);
});

test('mobile uses the shared local health module and runs its tests', () => {
  const module = readFileSync(new URL('../native/mobile/go.mod', import.meta.url), 'utf8');
  const runner = readFileSync(new URL('./mobile-native.mjs', import.meta.url), 'utf8');
  assert.match(module, /shellfox\.local\/tailscale-helper v0\.0\.0/);
  assert.match(module, /replace shellfox\.local\/tailscale-helper => \.\.\/tailscale/);
  assert.match(runner, /'\.\/\.\.\.', 'shellfox\.local\/tailscale-helper\/nethealth'/);
});

test('unknown targets and CLI commands fail before executing external tools', () => {
  assert.throws(() => bindingArgs('linux', 'wrong'), /android or ios/);
  assert.throws(() => main(['unknown']), /Usage/);
  assert.throws(() => main(['build', 'linux']), /android or ios/);
  assert.throws(() => main(['test', 'android', 'surprise']), /Usage/);
  if (process.platform !== 'darwin') assert.throws(() => main(['build', 'ios']), /macOS/);
});
