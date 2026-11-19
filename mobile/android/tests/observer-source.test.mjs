import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';

const root = new URL('../../../', import.meta.url);
const read = path => readFileSync(new URL(path, root), 'utf8');
const kotlin = name => read(`mobile/android/app/src/main/java/com/shellfox/mobile/${name}.kt`);
const rustKotlin = name => kotlin(`rust/${name}`);
const cpp = read('mobile/android/app/src/main/cpp/client_jni.cpp');
const profiles = read('mobile/android/app/src/main/cpp/client_profile_ops.h');
const header = read('crates/shellfox-mobile/include/shellfox_mobile_client.h');
const contract = JSON.parse(read('crates/shellfox-protocol/tests/fixtures/observer_v2/contract.json'));

// Source-role gates only. These never load JNI or the old staged gomobile AAR.
test('exact carrier paths and admission classes agree with canonical observer-v2', () => {
  const entries = [...kotlin('MobileEndpoint').matchAll(/V[12]_[A-Z]+\("([^"]+)", Traffic\.([A-Z]+)\)/g)];
  const roles = new Map(entries.map(([, path, role]) => [path, role]));
  assert.equal(entries.length, 10);
  assert.equal(roles.size, 10);
  for (const path of Object.values(contract.routes)) assert(roles.has(path));
  assert.deepEqual([...roles].filter(([, role]) => role === 'POLL').map(([path]) => path), ['/v1/events', '/v2/events']);
  assert.deepEqual([...roles].filter(([, role]) => role === 'PAIR' || role === 'DISCONNECT').map(([path]) => path),
    ['/v1/pair', '/v1/disconnect', '/v2/pair', '/v2/disconnect']);
  assert.match(kotlin('MobileEndpoint'), /it\.path == path/);
  assert.match(kotlin('MobileEndpoint'), new RegExp(`DEADLINE_MS = ${contract.limits.deadlineMs}`));
  assert.match(kotlin('ShellfoxNetworkModule'), /MobileEndpoint\.fromPath\(path\)/);
  assert.match(kotlin('ShellfoxNetworkModule'), /if \(endpoint\.isPoll\) polls else requests/);
  assert.match(kotlin('PairApprovalWakeLock'), /needsBackgroundCompletion/);
  assert.match(kotlin('PairApprovalWakeLock'), /coerceIn\(1L, 35000L\)/);
  assert.match(kotlin('PairApprovalWakeLock'), /finally .*wakeLock\.release/);
});

test('observer transport carries actual status; legacy completion stays distinct', () => {
  const transport = rustKotlin('NativeGoTransport');
  assert.match(transport, /@JvmStatic fun request\(/);
  assert.match(transport, /@JvmStatic fun requestHttp\(/);
  assert.match(transport, /Mobile\.requestWithStatus\(/);
  assert.match(transport, /received\.statusCode\(\)\.toLong\(\), received\.body\(\)/);
  assert.match(transport, /ClientBindings\.completeHttp\(/);
  assert.match(transport, /else ClientBindings\.complete\(/);
  assert.doesNotMatch(transport, /404|error\.message|catch[^\n]*Authentication/);
  assert.match(cpp, /client_ops::statusTransport\(ctx\.profile\) \? "requestHttp" : "request"/);
  assert.match(cpp, /client_ops::completeHttp\(ctx\.profile,/);
  assert.match(profiles, /profile==SFM_CLIENT_PROFILE_OBSERVER_V2\|\|profile==SFM_CLIENT_PROFILE_MANAGER_V2/);
  assert.match(profiles, /profile==SFM_CLIENT_PROFILE_OBSERVER_V2\)return sfm_observer_v2_complete_http\(/);
  assert.match(cpp, /sfm_client_complete\(/);
  assert.doesNotMatch(cpp, /path.*(?:startsWith|strncmp|404)/);
  const reply = rustKotlin('NativeHttpReply');
  assert.match(reply, /100L\.\.599L/);
  assert.match(reply, /status !in 200L\.\.299L && bytes\.isNotEmpty/);
  assert.doesNotMatch(reply, /404|permission|UNSUPPORTED|legacyAvailable/);
});

test('additive profile API matches header constants and never claims server grants', () => {
  assert.match(header, /SFM_OBSERVER_V2_ABI_VERSION 1/);
  assert.match(header, /SFM_CLIENT_PROFILE_V1 1/);
  assert.match(header, /SFM_CLIENT_PROFILE_OBSERVER_V2 2/);
  assert.match(header, /SFM_CLIENT_API_DELIVERY 1/);
  assert.match(header, /SFM_CLIENT_API_INPUT 2/);
  assert.match(header, /SFM_CLIENT_API_HOST_LAYOUT 4/);
  assert.match(rustKotlin('ClientProfile'), /V1\(1, "v1", 7\)/);
  assert.match(rustKotlin('ClientProfile'), /OBSERVER_V2\(2, "observer-v2", 1\)/);
  const store = rustKotlin('ClientTerminalBindings');
  assert.match(store, /requested: ClientProfile = ClientProfile\.V1/);
  assert.match(store, /ClientProfile\.decode\(ClientBindings\.profile\(handle\)\)/);
  assert.match(store, /"profile", it\.code/);
  assert.doesNotMatch(store, /System\.currentTimeMillis|"permission"|"controlEpoch"|"owned"/);
  for (const method of ['observerV2AbiVersion', 'clientProfile', 'createObserverV2Client', 'discoverObserverV2',
    'pairObserverV2', 'observerV2Operation', 'pollObserverV2', 'reconnectObserverV2', 'disconnectObserverV2']) {
    assert.match(read('mobile/src/rust-client/specs/NativeShellfoxRustClient.ts'), new RegExp(`${method}\\(`));
    assert.match(kotlin('ShellfoxRustClientModule'), new RegExp(`override fun ${method}\\(`));
  }
});

test('observer engine binding has native-consumption credit and no local parser fit', () => {
  for (const method of ['bind_terminal', 'unbind_terminal', 'terminal_reset_ticket', 'terminal_reset_grid',
    'terminal_reset_done', 'terminal_recovery_ready', 'terminal_consumed']) {
    assert.match(profiles, new RegExp(`\\bsfm_observer_v2_${method}\\b`));
  }
  assert.match(profiles, /profile==SFM_CLIENT_PROFILE_OBSERVER_V2\)return &observer/);
  assert.match(cpp, /client_ops::forProfile\(profile\)->bind\(handle,data\.data\(\),data\.size\(\),terminal\)/);
  const owner = rustKotlin('TerminalCoordinator');
  const resize = owner.slice(owner.indexOf('fun resize('), owner.indexOf('fun input('));
  assert.match(resize, /!entry\.recovering && entry\.profile\.allowsHostLayoutApi/);
  const fit = owner.slice(owner.indexOf('fun fitAfterRecovery('), owner.indexOf('fun scroll('));
  assert.match(fit, /if \(entry\.profile\.allowsHostLayoutApi\) \{[\s\S]*RustBindings\.resize/);
  assert.match(owner, /ClientTerminalBindings\.consumed\(id\)/);
  assert.match(rustKotlin('ClientTerminalBindings'), /ClientBindings\.resetTicket\(client\.handle, handle\)/);
  assert.match(rustKotlin('ClientTerminalBindings'), /ClientBindings\.recoveryReady\(client\.handle, nativeId\(terminal\)\)/);
  assert.match(rustKotlin('ShellfoxRustTerminalView'), /inputApiAllowed = profile\.allowsInputApi/);
});
