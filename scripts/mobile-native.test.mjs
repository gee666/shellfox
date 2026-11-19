import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { bindingArgs, bindingEnvironment, main, mobileVersion } from './mobile-native.mjs';

test('Android targets match the app universal 64-bit APK and minimum SDK', () => {
  const args = bindingArgs('android', '/repo/tmp/mobile-native/android/shellfox-mobile.aar');
  assert.ok(args.includes('-target=android/arm64,android/amd64'));
  assert.ok(args.includes('-androidapi=24'));
  assert.ok(args.includes('-trimpath'));
  assert.equal(args.at(-1), '.');
  assert.match(mobileVersion, /^v0\.0\.0-\d{14}-[a-f0-9]{12}$/);
});

test('Android builds append canonical 16 KiB ELF alignment without replacing caller flags', () => {
  const inherited = { CGO_ENABLED: '0', CGO_LDFLAGS: '-llog -Wl,--as-needed' };
  const before = { ...inherited };
  assert.deepEqual(bindingEnvironment('android', inherited), {
    CGO_ENABLED: '1', CGO_LDFLAGS: '-llog -Wl,--as-needed -Wl,-z,max-page-size=16384',
  });
  assert.deepEqual(bindingEnvironment('android', {}), {
    CGO_ENABLED: '1', CGO_LDFLAGS: '-Wl,-z,max-page-size=16384',
  });
  assert.deepEqual(inherited, before);
  const runner = readFileSync(new URL('./mobile-native.mjs', import.meta.url), 'utf8');
  assert.match(runner, /const buildEnv = bindingEnvironment\(target, env\)/);
});

test('iOS keeps inherited Mach-O flags and receives no Android ELF option', () => {
  const inherited = { CGO_LDFLAGS: '-framework Foundation' };
  const build = bindingEnvironment('ios', inherited);
  assert.deepEqual(build, { CGO_ENABLED: '1' });
  assert.equal({ ...inherited, ...build }.CGO_LDFLAGS, '-framework Foundation');
  assert.doesNotMatch(JSON.stringify(build), /max-page-size|16384/);
});

test('iOS binds both device and simulator without changing Mobile framework name', () => {
  const args = bindingArgs('ios', '/repo/tmp/mobile-native/ios/.staging/Mobile.xcframework');
  assert.ok(args.includes('-target=ios,iossimulator'));
  assert.ok(args.includes('-iosversion=15.1'));
  assert.equal(args[args.indexOf('-o') + 1], '/repo/tmp/mobile-native/ios/.staging/Mobile.xcframework');
});

const readSource = file => readFileSync(new URL(`../${file}`, import.meta.url), 'utf8')
  .replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/[^\n]*/g, '');
const v1Paths = ['/v1/discover', '/v1/pair', '/v1/request', '/v1/events', '/v1/disconnect'];
const v2Paths = Object.values(JSON.parse(readFileSync(
  new URL('../crates/shellfox-protocol/tests/fixtures/observer_v2/contract.json', import.meta.url), 'utf8')).routes);
const allowedPaths = [...v1Paths, ...v2Paths].sort();
const polls = ['/v1/events', '/v2/events'];
const pairAndDisconnect = ['/v1/pair', '/v2/pair', '/v1/disconnect', '/v2/disconnect'].sort();
const invalidPaths = ['/v1/events/', '/v2/events/', '/v2/events?x=1', '/V2/events',
  'https://host/v2/discover', '//v2/discover', '/v2/stop', '/v2/request\u0000'];
const objectivePaths = source => [...source.matchAll(/@"(\/v[12]\/[^"\n]+)"/g)].map(match => match[1]);
function objcPolicyFunction(source, name) {
  const body = source.match(new RegExp(`static inline BOOL ${name}\\(NSString \\*path\\) \\{([\\s\\S]*?)\\n\\}`));
  assert.ok(body, `missing actual ${name} definition`);
  return body[1];
}

test('Android source preserves exact ten carrier paths and bounded poll/background admission', () => {
  const base = 'mobile/android/app/src/main/java/com/shellfox/mobile/';
  const source = readSource(base + 'ShellfoxNetworkModule.kt');
  const helper = readSource(base + 'PairApprovalWakeLock.kt');
  const transport = readSource(base + 'rust/NativeGoTransport.kt');
  const policy = readSource(base + 'MobileEndpoint.kt');
  const entries = [...policy.matchAll(/^\s*V[12]_[A-Z]+\("([^"]+)", Traffic\.([A-Z]+)\)/gm)];
  const roles = new Map(entries.map(([, path, role]) => [path, role]));
  assert.equal(entries.length, 10);
  assert.deepEqual([...roles.keys()].sort(), allowedPaths);
  assert.deepEqual([...roles].filter(([, role]) => role === 'POLL').map(([path]) => path), polls);
  assert.deepEqual([...roles].filter(([, role]) => ['PAIR', 'DISCONNECT'].includes(role)).map(([path]) => path).sort(), pairAndDisconnect);
  assert.match(policy, /fromPath\(path: String\): MobileEndpoint\? = entries\.firstOrNull \{ it\.path == path \}/);
  assert.match(policy, /isPoll: Boolean get\(\) = traffic == Traffic\.POLL/);
  assert.match(policy, /needsBackgroundCompletion: Boolean get\(\) = traffic == Traffic\.PAIR \|\| traffic == Traffic\.DISCONNECT/);
  for (const path of invalidPaths) assert.equal(roles.has(path), false);
  assert.match(source, /if \(endpoint == null\) \{[\s\S]*?promise\.reject[\s\S]*?return/);
  for (const caller of [source, transport]) {
    assert.match(caller, /MobileEndpoint\.fromPath\(/);
    assert.match(caller, /if \(endpoint\.isPoll\) polls else requests/);
  }
  assert.match(readSource('mobile/android/app/src/main/AndroidManifest.xml'), /<uses-permission android:name="android\.permission\.WAKE_LOCK"\s*\/>/);
  assert.match(helper, /MobileEndpoint\.fromPath\(path\)\?\.needsBackgroundCompletion == true/);
  assert.match(helper, /newWakeLock\(PowerManager\.PARTIAL_WAKE_LOCK,/);
  assert.match(helper, /acquire\(deadlineMs\.coerceIn\(1L, 35000L\)\)/);
  assert.match(helper, /finally \{ if \(wakeLock\?\.isHeld == true\) wakeLock\.release\(\) \}/);
  assert.match(transport, /deadlines\.schedule\([\s\S]*?TimeUnit\.MILLISECONDS/);
  assert.match(transport, /deadlineMs !in 1\.\.MobileEndpoint\.DEADLINE_MS/);
});

test('iOS source preserves exact ten paths and finite poll/pair/disconnect completion', () => {
  const source = readSource('mobile/ios/ShellfoxMobile/ShellfoxNetwork.mm');
  const policy = readSource('mobile/ios/ShellfoxRust/SFNetworkPolicy.h');
  const allowed = objcPolicyFunction(policy, 'SFNetworkPathAllowed');
  const poll = objcPolicyFunction(policy, 'SFNetworkPathPoll');
  const finite = objcPolicyFunction(policy, 'SFNetworkPathFiniteCompletion');
  assert.deepEqual(objectivePaths(allowed).sort(), allowedPaths);
  assert.match(allowed, /isKindOfClass:NSString\.class/);
  assert.match(allowed, /containsObject:path/);
  assert.deepEqual(objectivePaths(poll), polls);
  assert.match(poll, /isEqualToString:@"\/v1\/events"\] \|\| \[path isEqualToString:@"\/v2\/events"/);
  assert.deepEqual(objectivePaths(finite).sort(), pairAndDisconnect);
  assert.match(finite, /SFNetworkPathPoll\(path\) \|\|/);
  assert.match(finite, /containsObject:path/);
  assert.doesNotMatch(policy, /hasPrefix|rangeOfString|lowercaseString|URLWithString/);
  const closed = new Set(objectivePaths(allowed));
  for (const path of invalidPaths) assert.equal(closed.has(path), false);
  assert.match(source, /if \(!SFNetworkPathAllowed\(path\)\) \{ reject\([\s\S]*?return; \}/);
  assert.match(source, /if \(SFNetworkPathFiniteCompletion\(path\)\)/);
  assert.match(source, /submitConcurrent:SFNetworkPathPoll\(path\)/);
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
  assert.match(runner, /runGoTests\(\{[^}]*\benv: \{ \.\.\.env, CGO_ENABLED: '0' \}, testTimeout: '60s',\s+packages: \['\.\/\.\.\.', 'shellfox\.local\/tailscale-helper\/nethealth'\]/);
});

test('unknown targets and CLI commands fail before executing external tools', () => {
  assert.throws(() => bindingArgs('linux', 'wrong'), /android or ios/);
  assert.throws(() => bindingEnvironment('linux', {}), /android or ios/);
  assert.throws(() => main(['unknown']), /Usage/);
  assert.throws(() => main(['build', 'linux']), /android or ios/);
  assert.throws(() => main(['test', 'android', 'surprise']), /Usage/);
  if (process.platform !== 'darwin') assert.throws(() => main(['build', 'ios']), /macOS/);
});
