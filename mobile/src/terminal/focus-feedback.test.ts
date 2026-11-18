import {expect,it} from 'vitest';
import {focusFeedbackSchema,focusNotice,focusActionNotice,focusResultSchema,isFocusFeedback} from './focus-feedback';
const common={source:'keyboard',attached:true,windowFocused:true,uiThread:true,inputAllowed:true,frameAvailable:true,focusRequest:'accepted',focused:true};
it('validates bounded Android result without equating IMM acceptance to visible keyboard',()=>{
 const result=focusResultSchema.parse({...common,platform:'android',keyboardRequest:'accepted'});
 expect(focusNotice(result)).toContain('snapshot focused=yes');expect('keyboardVisible'in result).toBe(false);
});
it('keeps iOS first-responder acceptance separate from keyboard display',()=>{
 const result=focusResultSchema.parse({...common,platform:'ios',keyboardRequest:'notApplicable'});
 expect(focusNotice(result)).toContain('snapshot focused=yes');expect('keyboardVisible'in result).toBe(false);
});
it('renders only a fixed compact failure notice',()=>{
 const result=focusResultSchema.parse({...common,platform:'android',focusRequest:'rejected',focused:false,keyboardRequest:'rejected'});
 expect(focusNotice(result)).toBe('android/keyboard: focus=rejected, snapshot focused=no, request=rejected; attached=yes, window=yes, UI=yes, input=yes, frame=yes.');
});
it('keeps SHOW/HIDE receipts separate from later observed snapshots without causal or visibility claims',()=>{
 expect(focusActionNotice({intent:'show',snapshot:null})).toBe('Keyboard SHOW. Snapshot since press: notObserved.');
 expect(focusActionNotice({intent:'hide',snapshot:null})).toBe('Keyboard HIDE. Snapshot since press: notObserved.');
 const snapshot=focusResultSchema.parse({...common,platform:'android',keyboardRequest:'accepted'});
 const notice=focusActionNotice({intent:'hide',snapshot});
 expect(notice).toContain('Keyboard HIDE. Snapshot since press: observed');expect(notice).toContain('snapshot focused=yes');
 expect(notice).not.toMatch(/visible|caused|granted|current focus/i);
});
it.each([
 {...common,platform:'other',keyboardRequest:'accepted'},
 {...common,platform:'android',source:'rpc',keyboardRequest:'accepted'},
 {...common,platform:'android',focusRequest:'forced',keyboardRequest:'accepted'},
 {...common,platform:'android',attached:'true',keyboardRequest:'accepted'},
 {...common,platform:'android',keyboardRequest:'notApplicable'},
 {...common,platform:'ios',keyboardRequest:'accepted'},
 {...common,platform:'android',keyboardRequest:'accepted',payload:'secret-command'},
 {...common,platform:'android',keyboardRequest:'accepted',token:'secret'},
])('rejects invalid enums, platform states, types and unexpected secret fields',value=>{
 expect(focusResultSchema.safeParse(value).success).toBe(false);
});
it('recognizes the exact codegen default object without treating it as focus feedback',()=>{
 const empty={platform:'',source:'',attached:false,windowFocused:false,uiThread:false,inputAllowed:false,frameAvailable:false,focusRequest:'',focused:false,keyboardRequest:''};
 expect(isFocusFeedback({state:'ready',focusResult:empty})).toBe(false);
 expect(isFocusFeedback({state:'focusResult',focusResult:empty})).toBe(true);
 expect(isFocusFeedback({state:'ready',focusResult:{...empty,payload:'forbidden'}})).toBe(true);
 expect(isFocusFeedback({state:'ready',focusResult:{...empty,platform:'android'}})).toBe(true);
});
it('validates the action discriminator before readiness and never returns the framework target',()=>{
 const body={terminalHandle:'100',state:'focusResult',focusResult:{...common,platform:'android',keyboardRequest:'accepted'}};
 expect(focusFeedbackSchema.safeParse(body).success).toBe(true);
 expect(focusFeedbackSchema.parse({...body,target:77})).not.toHaveProperty('target');
 expect(focusFeedbackSchema.safeParse({...body,state:'ready'}).success).toBe(false);
 expect(focusFeedbackSchema.safeParse({...body,rawRpc:'forbidden'}).success).toBe(false);
});
