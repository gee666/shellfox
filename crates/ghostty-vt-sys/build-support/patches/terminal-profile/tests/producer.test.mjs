import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { join } from 'node:path';
import { patchRoot, runtimeDigest } from '../tools/apply.mjs';
const checkpoint=JSON.parse(readFileSync(join(patchRoot,'checkpoint.json')));
const patch=readFileSync(join(patchRoot,checkpoint.patch),'utf8');
test('Native prerequisite checkpoint pins actual patch1 then complete patch2, never final artifacts',()=>{
 assert.equal(runtimeDigest(),checkpoint.patchSha256);
 const prerequisite=readFileSync(join(patchRoot,checkpoint.requires.patch));
 assert.equal(createHash('sha256').update(prerequisite).digest('hex'),checkpoint.requires.patchSha256);
 assert.equal(checkpoint.finalAcceptedEngineProfile,null);
 assert.match(checkpoint.status,/native-only.*not-final/);
});
test('patch2 consumes one authoritative checked Unicode API and does not overlay a table',()=>{
 assert.equal(existsSync(join(patchRoot,'overlay/src/terminal/shellfox_unicode_v6.zig')),false);
 assert.doesNotMatch(patch,/\+\+\+ b\/src\/terminal\/shellfox_unicode_v6\.zig/);
 assert.match(patch,/shellfox_unicode_v6\.properties\(c, self\.shellfox_preceding_join\) orelse return/);
 assert.doesNotMatch(patch,/shellfox_unicode_v6\.charProperties/);
});
test('profile ABI flip is real compiled API presence, not Legacy availability',()=>{
 assert.match(patch,/-pub const terminal_profile_abi_version: u32 = 0;\n\+pub const terminal_profile_abi_version: u32 = 1;/);
 assert.match(patch,/ghostty_shellfox_terminal_new_profile/);
 assert.match(patch,/pub const legacy_live_enabled = false;/);
 assert.equal(checkpoint.compiledABI.snapshot_budget_abi_version,0);
});
test('one sized State and no existing-terminal public setter',()=>{
 const header=readFileSync(join(patchRoot,'overlay/include/ghostty/vt/shellfox_terminal_profile.h'),'utf8');
 assert.match(header,/size_t size;\s+uint32_t abi_version;\s+uint32_t terminal_profile;\s+uint32_t preceding_join;\s*} GhosttyShellfoxTerminalProfileState;/);
 assert.doesNotMatch(header,/ghostty_shellfox_terminal_set/);
 assert.equal((patch.match(/if \(native\.shellfox_terminal_profile != 0\) return \.unsupported;/g)||[]).length,3);
});
test('revised Native002 candidate pins exact new prerequisite and sole capture bridge',()=>{
 const root=join(patchRoot,'checkpoints/native-002');const meta=JSON.parse(readFileSync(join(root,'checkpoint.json')));
 const bytes=readFileSync(join(root,meta.patch));assert.equal(createHash('sha256').update(bytes).digest('hex'),meta.patchSha256);
 assert.equal(createHash('sha256').update(readFileSync(join(root,meta.requires.patch))).digest('hex'),meta.requires.patchSha256);
 assert.match(bytes.toString('utf8'),/profiles\.capture\(t\) catch return T\.Reason\.profile/);
 assert.match(bytes.toString('utf8'),/captured\.terminal_profile == options\.target/);
 assert.equal(meta.tested.selectedTests,111);assert.equal(meta.finalAcceptedEngineProfile,null);
});
test('layout2 prototype preserves explicit failing status rather than advertise app availability',()=>{
 const root=join(patchRoot,'prototype');const meta=JSON.parse(readFileSync(join(root,'checkpoint.json')));
 assert.equal(createHash('sha256').update(readFileSync(join(root,meta.patch))).digest('hex'),meta.patchSha256);
 assert.equal(meta.legacyAppAvailability,false);assert.equal(meta.newSnapshotFormatImplemented,false);
 assert.match(meta.tested.latestFailure,/hidden|copy/);
});
test('retained baseline counterexamples prove copy and cursor gaps, not just widths',()=>{
 const report=JSON.parse(readFileSync(join(patchRoot,'tests/counterexamples.json')));
 const leading=report.cases.find(c=>c.name==='leading-combining');
 assert.notEqual(leading.xterm.copyLineHex,leading.native.copyLineHex);
 assert.notDeepEqual(leading.xterm.cursor,leading.native.cursor);
 const control=report.cases.find(c=>c.name==='control-reset-combining');
 assert.notDeepEqual(control.xterm.cursor,control.native.cursor);
});
