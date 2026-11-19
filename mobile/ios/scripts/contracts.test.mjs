import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { createRequire } from 'node:module';
import { after, test } from 'node:test';
import { minimumIOS, nativeLinkSettings, root, runtimePodspec, slices } from './build-rust.mjs';

const require = createRequire(import.meta.url);
const read = path => readFileSync(join(root, path), 'utf8');
mkdirSync(join(root, 'tmp'), { recursive: true });
const scratch = mkdtempSync(join(root, 'tmp/ios-contract-'));
after(() => rmSync(scratch, { recursive: true, force: true }));
const schema = join(scratch, 'schema.json');
function node(args) {
  const result = spawnSync(process.execPath, args, { cwd: root, encoding: 'utf8', timeout: 30000 });
  assert.equal(result.status, 0, result.stderr || result.stdout);
}
node(['node_modules/@react-native/codegen/lib/cli/combine/combine-js-to-schema-cli.js', schema,
  'mobile/src/rust-client/specs']);
node(['node_modules/react-native/scripts/generate-specs-cli.js', '-p', 'ios', '-s', schema,
  '-o', scratch, '-n', 'ShellfoxRustSpecs', '-t', 'all']);
const generated = path => readFileSync(join(scratch, path), 'utf8');
const source = readdirSync(join(root, 'mobile/ios/ShellfoxRust')).filter(p => /\.(mm|inc)$/.test(p))
  .map(p => read(`mobile/ios/ShellfoxRust/${p}`)).join('\n');

function selectors(text, ending) {
  const pattern = new RegExp(`-\\s*\\([^)]*\\)\\s*(\\w+)([\\s\\S]*?)${ending}`, 'g');
  return [...text.matchAll(pattern)].map(match => {
    const labels = [...(match[1] + match[2]).matchAll(/(\w+)\s*:/g)].map(m => m[1] + ':');
    return labels.length ? labels.join('') : match[1];
  });
}
test('installed RN generated ObjC selectors have native implementations', () => {
  const header = generated('ShellfoxRustSpecs/ShellfoxRustSpecs.h');
  const protocol = header.split('@protocol NativeShellfoxRustClientSpec')[1].split('@end')[0];
  const implemented = selectors(source, '\\{');
  for (const selector of selectors(protocol, ';')) assert.ok(implemented.includes(selector), `Missing ${selector}`);
  assert.match(source, /make_shared<facebook::react::NativeShellfoxRustClientSpecJSI>/);
  assert.match(header, /NSNumber \*\)abiVersion/);
});
test('installed RN providers resolve the actual native classes', () => {
  const pkg = JSON.parse(read('mobile/package.json'));
  assert.equal(pkg.codegenConfig.name, 'ShellfoxRustSpecs');
  const app = { config: pkg.codegenConfig, libraryPath: join(root, 'mobile') };
  const base = join(root, 'node_modules/react-native/scripts/codegen/generate-artifacts-executor');
  require(join(base, 'generateRCTModuleProviders.js')).generateRCTModuleProviders(join(root, 'mobile'), pkg, [], scratch);
  require(join(base, 'generateRCTThirdPartyComponents.js')).generateRCTThirdPartyComponents([app], scratch);
  assert.match(generated('RCTModuleProviders.mm'), /@"NativeShellfoxRustClient": @"ShellfoxRustClient"/);
  assert.match(generated('RCTThirdPartyComponentsProvider.mm'), /@"ShellfoxRustTerminal": NSClassFromString\(@"ShellfoxRustTerminalComponentView"\)/);
  assert.match(read('mobile/ios/ShellfoxMobile/SceneDelegate.swift'), /dependencyProvider = RCTAppDependencyProvider\(\)/);
  assert.match(source, /@implementation ShellfoxRustTerminalComponentView/);
  assert.match(source, /@implementation ShellfoxRustClient/);
});
test('native C calls exist in the actual Carter and client headers', () => {
  const headers = read('crates/shellfox-mobile/include/shellfox_mobile.h') + read('crates/shellfox-mobile/include/shellfox_mobile_client.h');
  const declarations = new Set([...headers.matchAll(/\b(sfm_\w+)\s*\(/g)].map(m => m[1]));
  for (const call of source.matchAll(/\b(sfm_\w+)\b/g)) assert.ok(declarations.has(call[1]), call[1]);
  assert.match(headers, /sfm_client_terminal_reset_done\(SfmClientHandle client,\s*SfmHandle terminal, uint64_t ticket\)/);
  assert.match(source, /\[client functions\]\.resetTicket\(client.handle, terminal.handle, &ticket\)/);
  assert.match(source, /\[client functions\]\.resetDone\(client.handle, terminal.handle, ticket\)/);
  assert.match(source, /\[client functions\]\.resetGrid\(client.handle, terminal.handle, ticket, &hostCols, &hostRows\)/);
  assert.match(source, /\[client functions\]\.ready\(client.handle, terminal.handle, ready\)/);
});
test('Rust input hints stay paired with explicit-length bytes through admission and Go writes', () => {
  const owner = read('mobile/ios/ShellfoxRust/SFOwner.mm');
  const input = read('mobile/ios/ShellfoxRust/SFOwnerInput.mm');
  const store = read('mobile/ios/ShellfoxRust/SFClientStore.mm');
  const queue = read('mobile/ios/ShellfoxRust/SFIntentQueue.mm');
  assert.match(input, /sfm_terminal_input_ex\(terminal.handle, \(const uint8_t \*\)json.bytes, json.length, CopyInputEx, &encoded\)/);
  assert.match(input, /packet.userInitiated = hint/);
  assert.match(owner, /sendInput:data userInitiated:intent.userInitiated/);
  assert.match(store, /sfm_client_terminal_write_ex\(client.handle, terminal.handle, \(const uint8_t \*\)bytes.bytes, bytes.length, hint, &operation\)/);
  assert.match(queue, /copy.userInitiated = intent.userInitiated/);
  assert.match(queue, /tail.userInitiated == intent.userInitiated/);
  assert.doesNotMatch(source, /\bsfm_terminal_input\s*\(|\bsfm_client_terminal_write\s*\(/);
  const fixture = read('mobile/ios/ShellfoxRust/Tests/SFInputPipelineTests.mm');
  assert.match(fixture, /testCtrlSpaceNulAndPassiveHintReachGoBoundaryWithoutReencoding/);
  assert.match(fixture, /initWithRequestFunction/);
  assert.match(fixture, /characterAtIndex:0/);
});
test('Fabric uses generated descriptors, props and coarse event types only', () => {
  const props = generated('react/renderer/components/ShellfoxRustSpecs/Props.h');
  const events = generated('react/renderer/components/ShellfoxRustSpecs/EventEmitters.h');
  assert.match(props, /int modifiers\{0\}/);
  assert.match(props, /Float fontSize/);
  assert.match(props, /bool focused\{false\}/);
  assert.match(props, /bool selecting\{false\}/);
  assert.match(events, /void onInput\(OnInput value\)/);
  assert.match(events, /void onSelection\(OnSelection value\)/);
  assert.match(events, /std::string terminalHandle/);
  assert.match(events, /int code/);
  assert.doesNotMatch(source, /WKWebView|RCTViewManager|registerComponentViewClass|onFrame|onCells/);
  const native = read('mobile/ios/ShellfoxRust/ShellfoxRustTerminalComponentView.mm');
  assert.match(native, /concreteComponentDescriptorProvider<ShellfoxRustTerminalComponentDescriptor>/);
  assert.match(native, /static_pointer_cast<const ShellfoxRustTerminalEventEmitter>/);
});
test('all iOS slices and Pods retain the installed RN minimum', () => {
  assert.equal(minimumIOS(), '15.1');
  assert.deepEqual(slices.map(s => s.target), ['aarch64-apple-ios', 'aarch64-apple-ios-sim', 'x86_64-apple-ios']);
  assert.ok(slices.every(s => s.zig.includes('.15.1')));
  assert.match(read('mobile/ios/Podfile'), /platform :ios, min_ios_version_supported/);
  assert.match(read('mobile/ios/Podfile'), /pod 'ShellfoxRustNative'/);
  assert.match(read('mobile/ios/ShellfoxRust/ShellfoxRustNative.podspec'), /install_modules_dependencies\(s\)/);
  assert.match(runtimePodspec(), /s.platform = :ios, '15.1'/);
  const versions = [...read('mobile/ios/ShellfoxMobile.xcodeproj/project.pbxproj').matchAll(/IPHONEOS_DEPLOYMENT_TARGET = ([^;]+);/g)];
  assert.ok(versions.length > 0 && versions.every(m => m[1] === '15.1'));
});
test('runtime Pod linker settings come from actual rustc output, not guessed frameworks', () => {
  const link = nativeLinkSettings('note: native-static-libs: -lSystem -lc -liconv -framework CoreFoundation -lc\n');
  assert.deepEqual(link, { libraries: ['System', 'c', 'iconv'], frameworks: ['CoreFoundation'] });
  assert.match(runtimePodspec(link), /s.libraries = 'System', 'c', 'iconv'/);
  assert.match(runtimePodspec(link), /s.frameworks = 'CoreFoundation'/);
  assert.throws(() => nativeLinkSettings('no native link output'));
  assert.throws(() => nativeLinkSettings('native-static-libs: -Wl,unexpected-option'));
});
test('observer profile is explicit, delivery-only and keeps the canonical native parser grid', () => {
  const module = read('mobile/ios/ShellfoxRust/ShellfoxRustClient.mm');
  const store = read('mobile/ios/ShellfoxRust/SFClientStore.mm');
  const polling = read('mobile/ios/ShellfoxRust/SFClientPolling.mm');
  const owner = read('mobile/ios/ShellfoxRust/SFOwnerInput.mm');
  const table = read('mobile/ios/ShellfoxRust/SFClientProfile.mm');
  assert.match(module, /observerV2Operation:/);
  assert.match(module, /sfm_client_profile\(client.handle, &profile, &capabilities\)/);
  assert.doesNotMatch(module, /sfm_client_profile\(terminal/);
  assert.match(store, /profile:SFM_CLIENT_PROFILE_V1 result:result/);
  assert.match(store, /profile:SFM_CLIENT_PROFILE_OBSERVER_V2 result:result/);
  assert.match(table, /SFM_CLIENT_PROFILE_OBSERVER_V2, SFM_CLIENT_API_DELIVERY/);
  for (const api of ['bind_terminal', 'unbind_terminal', 'terminal_reset_ticket', 'terminal_reset_grid',
    'terminal_reset_done', 'terminal_recovery_ready', 'terminal_consumed', 'poll', 'disconnect']) {
    assert.ok(table.includes(`sfm_observer_v2_${api}`));
  }
  const resize = owner.split('- (SfmStatus)resize:')[1].split('- (SfmStatus)input:')[0];
  assert.ok(resize.indexOf('if (terminal.deliveryOnly) return SFM_OK;') < resize.indexOf('sfm_terminal_resize('));
  assert.match(owner, /if \(terminal.deliveryOnly\) return SFM_INVALID/);
  assert.match(polling, /connection\[@"binding"\]/);
  assert.match(polling, /@"observer-v2"/);
});
test('both native carriers use exact routes and preserve real HTTP status without native fallback', () => {
  const policy = read('mobile/ios/ShellfoxRust/SFNetworkPolicy.h');
  const allow = policy.split('static inline BOOL SFNetworkPathPoll')[0];
  const paths = [...allow.matchAll(/@"(\/v[12]\/[^\"]+)"/g)].map(m => m[1]).sort();
  const expected = [1, 2].flatMap(version => ['discover', 'pair', 'request', 'events', 'disconnect']
    .map(name => `/v${version}/${name}`)).sort();
  assert.deepEqual(paths, expected);
  const goPolicy = [...read('native/mobile/policy.go').matchAll(/"(\/v[12]\/[^\"]+)"/g)].map(m => m[1]);
  assert.deepEqual([...new Set(goPolicy)].sort(), expected);
  const transport = read('mobile/ios/ShellfoxRust/SFGoTransport.mm');
  assert.match(transport, /@implementation SFGoHTTPResponse[\s\S]*?@end\s*@interface SFGoRequest/);
  const spec = read('mobile/ios/ShellfoxRust/ShellfoxRustNative.podspec');
  assert.match(spec, /'CLANG_ENABLE_MODULES' => 'YES'/);
  assert.match(spec, /-fcxx-modules/);
  const legacy = read('mobile/ios/ShellfoxMobile/ShellfoxNetwork.mm');
  for (const text of [transport, legacy]) {
    assert.match(text, /SFNetworkPathPoll\(path\)/);
    assert.match(text, /SFNetworkPathFiniteCompletion\(path\)/);
  }
  assert.match(legacy, /SFNetworkPathAllowed\(path\)/);
  assert.match(transport, /MobileRequestWithStatus\(peer, path, body, error\)/);
  assert.match(transport, /\[result statusCode\]/);
  assert.match(transport, /\[result body\]/);
  assert.match(transport, /sfm_observer_v2_complete_http\(client, generation, request, outcome, status/);
  assert.doesNotMatch(transport, /404|localizedDescription|rangeOfString|containsString/);
  const fixture = read('mobile/ios/ShellfoxRust/Tests/SFObserverV2Tests.mm');
  assert.match(fixture, /sequence:UINT64_MAX/);
  assert.match(fixture, /testOnlyActualDiscover404OffersLegacyAndNeverPairsAutomatically/);
});
test('observer native fixtures retain canonical golden bytes without v1 DTO conversions', () => {
  for (const name of ['discovery', 'pair-selection', 'attachment', 'events', 'acknowledged', 'request-attach']) {
    assert.equal(read(`mobile/ios/ShellfoxRust/Tests/Fixtures/observer-v2-${name}.json`),
      read(`crates/shellfox-protocol/tests/fixtures/observer_v2/${name}.json`));
  }
  const attachment = JSON.parse(read('mobile/ios/ShellfoxRust/Tests/Fixtures/observer-v2-attachment.json'));
  assert.equal(attachment.value.kind, 'attached');
  assert.equal(attachment.value.input, null);
  assert.equal(attachment.value.layout.kind, 'fixedCanonicalGrid');
  assert.equal(Object.hasOwn(attachment.value, 'control'), false);
});
test('native source files stay within the project size limit', () => {
  for (const name of readdirSync(join(root, 'mobile/ios/ShellfoxRust'))) {
    if (!/\.(h|mm|inc)$/.test(name)) continue;
    assert.ok(read(`mobile/ios/ShellfoxRust/${name}`).split('\n').length <= 350, name);
  }
});
