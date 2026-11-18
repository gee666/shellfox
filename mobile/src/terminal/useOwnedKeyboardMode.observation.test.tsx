// @vitest-environment jsdom
import {act,renderHook} from '@testing-library/react';
import {beforeEach,describe,expect,it,vi} from 'vitest';
import type {NativeViewDriver,NativeViewSession} from '../rust-client/native-terminal';
import {terminalHandleSchema} from '../rust-client/adapter';
import type {TerminalBridgeState} from './bridge';
import {keyboardClearReasonSchema} from './keyboard-retirement';
import {useOwnedKeyboardMode} from './useOwnedKeyboardMode';
const calls=vi.hoisted(()=>({dismiss:vi.fn()}));
vi.mock('react-native',()=>({Keyboard:{dismiss:calls.dismiss}}));
function fixture(){
  const identity={tabId:'20000000-0000-4000-8000-000000000001',generation:'30000000-0000-4000-8000-000000000001',controlEpoch:1};
  let liveIdentity:typeof identity|null=identity,state:TerminalBridgeState='attached';
  const view:NativeViewSession={tabId:identity.tabId,handle:terminalHandleSchema.parse('100'),readOnly:false,
    getState:vi.fn(()=>state),getIdentity:vi.fn(()=>liveIdentity),nativeState:vi.fn(),
    subscribe:()=>()=>undefined,subscribeError:()=>()=>undefined,configure:vi.fn(async()=>undefined),
    input:vi.fn(async()=>undefined),takeControl:vi.fn(async()=>undefined),close:vi.fn(async()=>undefined)};
  const driver:NativeViewDriver={open:vi.fn(async()=>view)};
  const current:{current:NativeViewSession|null}={current:view};
  const context={view,current,driver,tabId:identity.tabId,connectionEpoch:'original',readOnly:false};
  const rendered=renderHook(({scope,visible})=>useOwnedKeyboardMode(scope,visible),{initialProps:{scope:context,visible:false}});
  return{...rendered,context,view,driver,originalIdentity:identity,
    setIdentity:(next:typeof identity|null)=>{liveIdentity=next;},
    bridge:(next:TerminalBridgeState)=>{state=next;},
    visibility:(visible:boolean)=>rendered.rerender({scope:context,visible})};
}
beforeEach(()=>{vi.clearAllMocks();});
describe('first-only retirement observation does not drive policy',()=>{
  it.each(keyboardClearReasonSchema.options)('records one bounded %s reason only after accepted action',reason=>{
    const f=fixture();act(()=>{expect(f.result.current.choose('system','show')).toBe(true);});
    act(()=>f.result.current.clear(true,reason));
    expect(f.result.current.retirement).toEqual({intent:'show',reason});expect(f.result.current.mode).toBe('closed');
    act(()=>f.result.current.clear(true,'native_error','binding_retired'));
    expect(f.result.current.retirement).toEqual({intent:'show',reason});expect(calls.dismiss).toHaveBeenCalledTimes(2);
    expect(f.view.input).not.toHaveBeenCalled();expect(f.driver.open).not.toHaveBeenCalled();f.unmount();
  });
  it('does not record or expose an action when there is no accepted keyboard action',()=>{
    const f=fixture();act(()=>f.result.current.clear(true,'native_error'));
    expect(f.result.current.retirement).toBeNull();expect(f.result.current.actionIntent).toBeNull();
    f.setIdentity(null);act(()=>{expect(f.result.current.choose('system','show')).toBe(false);});
    act(()=>f.result.current.clear(false,'unknown'));expect(f.result.current.retirement).toBeNull();f.unmount();
  });
  it('resets on a new accepted action while preserving explicit HIDE semantics',()=>{
    const f=fixture();act(()=>f.result.current.choose('system','show'));act(()=>f.result.current.clear(false,'visibility_hidden_after_ready'));
    expect(f.result.current.retirement?.reason).toBe('visibility_hidden_after_ready');
    act(()=>f.result.current.choose('system','show'));expect(f.result.current.retirement).toBeNull();
    act(()=>f.result.current.choose('closed','hide'));expect(f.result.current.retirement).toEqual({intent:'hide',reason:'explicit_mode_close'});
    expect(f.result.current.mode).toBe('closed');expect(calls.dismiss).toHaveBeenCalledTimes(1);f.unmount();
  });
  it('keeps valid native recovery and ignored visibility changes free of retirement records',()=>{
    const f=fixture();act(()=>f.result.current.choose('system','show'));f.visibility(true);
    act(()=>{expect(f.result.current.recovering(f.view)).toBe(true);});f.setIdentity(null);f.visibility(false);
    expect(f.result.current.mode).toBe('system');expect(f.result.current.retirement).toBeNull();expect(calls.dismiss).not.toHaveBeenCalled();
    f.setIdentity(f.originalIdentity);f.unmount();
  });
  it('records ready rejection and ordinary hide at their own source branches',()=>{
    const f=fixture();act(()=>f.result.current.choose('system','show'));act(()=>f.result.current.recovering(f.view));
    f.setIdentity(null);act(()=>f.result.current.ready(f.view));expect(f.result.current.retirement?.reason).toBe('ready_context_lost');
    f.unmount();const g=fixture();act(()=>g.result.current.choose('system','show'));g.visibility(true);g.visibility(false);
    expect(g.result.current.retirement?.reason).toBe('visibility_hidden_after_ready');expect(g.result.current.mode).toBe('closed');g.unmount();
  });
  it('records invalid local request context at the existing validity effect without new work',()=>{
    const f=fixture();act(()=>f.result.current.choose('system','show'));
    f.setIdentity({...f.originalIdentity,controlEpoch:2});f.rerender({scope:f.context,visible:false});
    expect(f.result.current.retirement?.reason).toBe('request_context_invalid');expect(f.result.current.mode).toBe('closed');
    expect(calls.dismiss).toHaveBeenCalledTimes(1);expect(f.view.input).not.toHaveBeenCalled();f.unmount();
  });
  it('publishes the first retirement before a reentrant dismissal callback and retains ordinary mode order',()=>{
    const f=fixture();act(()=>f.result.current.choose('system','show'));
    calls.dismiss.mockImplementationOnce(()=>f.result.current.clear(false,'native_error'));
    act(()=>f.result.current.choose('closed','hide'));
    expect(f.result.current.retirement).toEqual({intent:'hide',reason:'explicit_mode_close'});expect(f.result.current.mode).toBe('closed');
    expect(calls.dismiss).toHaveBeenCalledTimes(1);act(()=>f.result.current.choose('system','show'));
    expect(f.result.current.retirement).toBeNull();expect(f.result.current.mode).toBe('system');f.unmount();
  });
  it('records layout loss only after the pending notice belongs to the accepted original action',()=>{
    const f=fixture();act(()=>f.result.current.choose('system','show'));f.bridge('reconnecting');f.setIdentity(null);
    act(()=>{expect(f.result.current.layoutPending(f.view,{kind:'layoutRecovery',phase:'pending',identity:f.originalIdentity})).toBe(true);});
    f.bridge('attached');act(()=>f.result.current.layoutFinished(f.view,{kind:'layoutRetired',outcome:'response_control_changed'}));
    expect(f.result.current.retirement).toEqual({intent:'show',reason:'layout_validation_lost',comparison:'response_control_changed'});
    act(()=>f.result.current.clear(true,'session_error'));expect(f.result.current.retirement?.reason).toBe('layout_validation_lost');f.unmount();
  });
  it('does not let a stale revision reset, retire or annotate the newer action',()=>{
    const f=fixture();const old=f.result.current.choose;act(()=>f.result.current.choose('system','show'));
    act(()=>f.result.current.clear(true,'layout_validation_lost','response_control_changed'));
    act(()=>f.result.current.choose('system','show'));expect(f.result.current.retirement).toBeNull();
    act(()=>{expect(old('system','show')).toBe(false);expect(old('closed','hide')).toBe(false);});
    expect(f.result.current.mode).toBe('system');expect(f.result.current.actionIntent).toBe('show');expect(f.result.current.retirement).toBeNull();f.unmount();
  });
  it('suppresses old action data at a new lifecycle/context and cannot observe a destroyed destination',()=>{
    const f=fixture();act(()=>f.result.current.choose('system','show'));act(()=>f.result.current.clear(true,'view_effect_cleanup'));
    expect(f.result.current.retirement?.reason).toBe('view_effect_cleanup');
    f.rerender({scope:{...f.context,connectionEpoch:'replacement'},visible:false});expect(f.result.current.retirement).toBeNull();expect(f.result.current.actionIntent).toBeNull();
    f.unmount();expect(f.view.input).not.toHaveBeenCalled();const g=fixture();expect(g.result.current.retirement).toBeNull();g.unmount();
  });
});
