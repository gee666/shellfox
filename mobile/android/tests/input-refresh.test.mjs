import fs from 'node:fs';
import assert from 'node:assert/strict';
import {test} from 'node:test';
const read=file=>fs.readFileSync(new URL('../../../'+file,import.meta.url),'utf8');
const path='mobile/android/app/src/main/java/com/shellfox/mobile/rust/';
test('Android frame and re-enable share the private current-context refresh gate',()=>{
 const view=read(path+'ShellfoxRustTerminalView.kt');
 const setter=view.slice(view.indexOf('  var readOnly'),view.indexOf('  private var inputApiAllowed'));
 assert.match(setter,/connection\?\.cancelComposition\(\); input.reset\(\)/);assert.match(setter,/refreshInputContext\(\)/);
 const frame=view.slice(view.indexOf('    override fun frame('),view.indexOf('    override fun state('));
 assert.match(frame,/inputEpoch == originalEpoch && inputReset === originalReset && inputAvailable/);
 assert.match(frame,/sink === this/);assert.match(frame,/refreshInputContext\(\)/);
 const refresh=view.slice(view.indexOf('  private fun inputRefreshState'),view.indexOf('  override fun onDraw'));
 assert.match(refresh,/active.inputAvailable && active.frames.acquire\(\) != null/);
 assert.match(refresh,/isAttachedToWindow, hasWindowFocus\(\), isFocused, acceptsInput/);
 assert.doesNotMatch(refresh,/requestFocus|showSoftInput|TerminalCoordinator\.input|ClientBindings|RustBindings/);
});
test('posted refresh rechecks the original context and never retries an uncertain episode',()=>{
 const gate=read(path+'InputRefreshGate.kt');
 assert.match(gate,/sink === other.sink && epoch == other.epoch && reset === other.reset/);
 assert.match(gate,/episode !== original/);assert.match(gate,/now\?\.eligible != true/);
 assert.match(gate,/original.original.sameContext\(now\)/);
 assert.match(gate,/original.outcome = Outcome.ATTEMPTED[\s\S]*restart\(\)/);
 assert.match(gate,/Outcome.UNKNOWN; lastUnknown = original/);
 assert.doesNotMatch(gate,/requestFocus|showSoftInput|Handler|postDelayed|Timer|sfm_|RustBindings|ClientBindings/);
});
test('presentation focus does not remove any native input eligibility, epoch or SDK lane fence',()=>{
 const view=read(path+'ShellfoxRustTerminalView.kt');
 assert.match(view,/acceptsInput: Boolean get\(\) = !readOnly && inputApiAllowed/);
 assert.match(view,/acceptsInput && isFocused && active != null && sink === active && active.inputAvailable && active.inputEpoch == epoch/);
 assert.match(view,/\(acceptsInput && input.hardware\(event\)\) \|\| super.onKeyDown/);
 const owner=read(path+'TerminalCoordinator.kt');assert.match(owner,/entry.inputEpoch != epoch \|\| entry.recovering/);
 const bindings=read(path+'ClientTerminalBindings.kt');assert.match(bindings,/resetting.contains\(terminal\) \|\| recovering.contains\(terminal\)/);
 const ui=read('mobile/src/terminal/RustTerminalView.tsx');assert.match(ui,/readOnly=\{!inputEnabled\} focused=\{keyboardMode === 'system'\}/);
 assert.match(ui,/inputEnabled = !readOnly && nativeReady && state === 'attached' && session\?\.getIdentity\(\) != null/);
});
