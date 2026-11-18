import fs from 'node:fs';
import assert from 'node:assert/strict';
import {test} from 'node:test';
const read=file=>fs.readFileSync(new URL('../../../'+file,import.meta.url),'utf8');
test('Android observes each existing focus action once and fences its posted UI result',()=>{
 const view=read('mobile/android/app/src/main/java/com/shellfox/mobile/rust/ShellfoxRustTerminalView.kt');
 const action=view.slice(view.indexOf('  private fun focusAction'),view.indexOf('  fun scroll('));
 assert.equal((action.match(/requestFocus\(\)/g)||[]).length,1);
 assert.equal((action.match(/showSoftInput\(/g)||[]).length,1);
 assert.match(action,/fence.current\(terminalHandle, sink, sink\?\.inputEpoch\)/);
 assert.doesNotMatch(action,/TerminalCoordinator\.(input|resize|attach|consumed)|RustBindings|ClientBindings/);
 const payload=read('mobile/android/app/src/main/java/com/shellfox/mobile/rust/TerminalFocusFeedback.kt');
 assert.doesNotMatch(payload.slice(payload.indexOf('fun map')),/put(?:String|Int|Double)\("(?:epoch|sink|tag|key|command|url|token|payload)"/);
});
test('iOS observes existing first-responder request without claiming keyboard visibility or private category ivars',()=>{
 const source=read('mobile/ios/ShellfoxRust/SFTerminalFocus.mm');
 assert.equal((source.match(/becomeFirstResponder\]/g)||[]).length,1);
 assert.match(source,/attempt \? \[_input becomeFirstResponder\] : NO/);
 assert.match(source,/@"keyboardRequest":@"notApplicable"/);
 assert.match(source,/view->_binding != binding/);
 assert.match(source,/view->_mailbox != mailbox/);
 assert.match(source,/mailbox->epoch.load\(\) != epoch/);
 assert.doesNotMatch(source,/_parserReady|stateChanged|sfm_|sendInput|sendCanonicalInput/);
});
test('focus action feedback routes before readiness and cannot grant readiness or SDK credit',()=>{
 const view=read('mobile/src/terminal/RustTerminalView.tsx');
 const start=view.indexOf('if(isFocusFeedback(value))');
 const end=view.indexOf('const parsed = nativeTerminalStateSchema',start);
 assert.ok(start>=0&&end>start);
 const branch=view.slice(start,end);
 assert.match(branch,/focusFeedbackSchema.safeParse/);assert.match(branch,/return;/);
 assert.doesNotMatch(branch,/setNativeReady|\.nativeState\(|\.input\(|\.poll\(|acknowledge|\.attach\(/);
});
