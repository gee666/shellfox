// @vitest-environment jsdom
import React, {type ReactNode} from 'react';
import {act, cleanup, fireEvent, render, screen} from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import {afterEach, beforeEach, describe, expect, it, vi} from 'vitest';
import {RustTerminalView} from './RustTerminalView';
import type {NativeViewDriver, NativeViewSession, NativeViewStateListener} from '../rust-client/native-terminal';
import {terminalHandleSchema} from '../rust-client/adapter';
import type {TerminalViewState} from './terminal-view-props';
import type {UploadIdentity} from './upload';

interface UiProps {children?: ReactNode; accessibilityLabel?: string; onPress?: () => void; disabled?: boolean; accessibilityState?: {selected?: boolean}}
interface NativeProbe {
  terminalHandle: string; readOnly?: boolean; focused?: boolean; modifiers?: number;
  onState?: (event: {nativeEvent: {terminalHandle: string; state: string}}) => void;
  onError?: (event: {nativeEvent: {terminalHandle: string; code: number}}) => void;
}
const probe = vi.hoisted(() => ({platform:'android' as 'android'|'ios', visible:false,
  native:undefined as NativeProbe|undefined, focusLog:[] as boolean[], keyboardPress:undefined as (()=>void)|undefined,
  invalidate:vi.fn(), dismiss:vi.fn()}));
vi.mock('react-native', () => ({
  Platform:{get OS(){return probe.platform;}}, StyleSheet:{create:<T,>(styles:T)=>styles},
  Keyboard:{dismiss:probe.dismiss}, Alert:{alert:vi.fn()},
  View:({children}:UiProps)=><div>{children}</div>, Text:({children}:UiProps)=><span>{children}</span>,
  ScrollView:({children}:UiProps)=><div>{children}</div>, KeyboardAvoidingView:({children}:UiProps)=><div>{children}</div>,
  Modal:({children,visible}:UiProps&{visible:boolean})=>visible?<div>{children}</div>:null,
  TextInput:()=>null,
  Pressable:({children,onPress,accessibilityLabel,accessibilityState,disabled}:UiProps)=>{
    if(accessibilityLabel==='Keyboard')probe.keyboardPress=onPress;
    return <button aria-label={accessibilityLabel} aria-pressed={accessibilityState?.selected} disabled={disabled} onClick={onPress}>{children}</button>;
  },
}));
vi.mock('react-native-safe-area-context',()=>({useSafeAreaInsets:()=>({bottom:20})}));
vi.mock('./useKeyboardOverlap',()=>({useKeyboardOverlap:()=>({keyboardInset:0,keyboardOffset:0,
  keyboardVisible:probe.visible,measureKeyboardInset:()=>undefined})}));
vi.mock('./useTerminalAttachment',()=>({useTerminalAttachment:()=>({invalidate:probe.invalidate,panel:null,button:undefined})}));
vi.mock('@react-native-clipboard/clipboard',()=>({default:{setString:vi.fn(),getString:async()=>''}}));
vi.mock('../rust-client/specs/ShellfoxRustTerminalNativeComponent',()=>({default:(props:NativeProbe)=>{
  probe.native=props;probe.focusLog.push(Boolean(props.focused));
  return <div aria-label="Native terminal" data-readonly={String(props.readOnly)} data-focused={String(props.focused)} data-modifiers={props.modifiers}>
    {['bound','ready','recovering','closed','invalid'].map(state=><button key={state} aria-label={`Native state ${state}`}
      onClick={()=>props.onState?.({nativeEvent:{terminalHandle:props.terminalHandle,state}})}/>)}
  </div>;
}}));
let nextHandle=100;
function ownedView(tabId:string,readOnly:boolean){
  let identity:UploadIdentity={tabId,generation:'30000000-0000-4000-8000-000000000001',controlEpoch:1};
  let parserReady=false,closed=false,owned=true;
  let bridgeState:TerminalViewState='attached';
  const listeners=new Set<NativeViewStateListener>();
  let lastListener:NativeViewStateListener|undefined;
  const errors=new Set<(error:Error)=>void>();
  const session:NativeViewSession={tabId,handle:terminalHandleSchema.parse(String(nextHandle++)),readOnly,
    getState:()=>bridgeState,nativeState:vi.fn(value=>{parserReady=value==='ready';}),
    getIdentity:()=>!closed&&!readOnly&&owned&&bridgeState==='attached'&&parserReady?identity:null,
    subscribe:listener=>{lastListener=listener;listeners.add(listener);return()=>{listeners.delete(listener);};},
    subscribeError:listener=>{errors.add(listener);return()=>{errors.delete(listener);};},
    configure:vi.fn(async()=>undefined),takeControl:vi.fn(async()=>undefined),input:vi.fn(async()=>undefined),
    close:vi.fn(async()=>{closed=true;}),
  };
  return {session,originalIdentity:{...identity},stateCallback:()=>lastListener,
    ownership:(allowed:boolean)=>{act(()=>{owned=allowed;});},
    owner:(next:Partial<UploadIdentity>,mutate=false)=>{act(()=>{identity=mutate?Object.assign(identity,next):{...identity,...next};});},
    bridge:(next:TerminalViewState)=>{act(()=>{bridgeState=next;for(const listener of listeners)listener(next);});},
    error:()=>{act(()=>{for(const listener of errors)listener(new Error('Fixture view failed'));});},
  };
}
function makeDriver(){
  const views:ReturnType<typeof ownedView>[]=[];
  const driver:NativeViewDriver={open:vi.fn(async(tabId,readOnly)=>{const view=ownedView(tabId,readOnly);views.push(view);return view.session;})};
  return {driver,views};
}
async function fixture(){
  const host=makeDriver();
  let props={driver:host.driver,tabId:'20000000-0000-4000-8000-000000000001',connectionEpoch:'original',readOnly:false};
  const element=()=><RustTerminalView {...props} fontSize={14} showKeyboardDiagnostics/>;
  const rendered=render(element());
  const flush=async()=>{await act(async()=>{await Promise.resolve();await Promise.resolve();});};
  await flush();
  const refresh=()=>rendered.rerender(element());
  const install=async(next:Partial<typeof props>)=>{props={...props,...next};refresh();await flush();};
  const visibility=(visible:boolean)=>{probe.visible=visible;refresh();};
  state('ready');
  return {...rendered,host,view:host.views[0],terminal:screen.getByLabelText('Native terminal'),install,visibility,refresh};
}
function state(value:string){fireEvent.click(screen.getByRole('button',{name:`Native state ${value}`}));}
function keyboard(){fireEvent.click(screen.getByRole('button',{name:'Keyboard',exact:true}));}
function focused(value:boolean){expect(screen.getByLabelText('Native terminal')).toHaveAttribute('data-focused',String(value));}
function readonly(value:boolean){expect(screen.getByLabelText('Native terminal')).toHaveAttribute('data-readonly',String(value));}
function noWork(value:Awaited<ReturnType<typeof fixture>>){
  expect(value.host.driver.open).toHaveBeenCalledTimes(1);expect(value.view.session.close).not.toHaveBeenCalled();
  expect(value.view.session.input).not.toHaveBeenCalled();expect(value.view.session.takeControl).not.toHaveBeenCalled();
}
beforeEach(()=>{vi.useFakeTimers();vi.clearAllMocks();probe.visible=false;probe.native=undefined;probe.keyboardPress=undefined;nextHandle=100;});
afterEach(()=>{cleanup();vi.useRealTimers();});
for(const platform of ['android','ios'] as const){
  describe(`owned keyboard desire, ${platform} shared source`,()=>{
    beforeEach(()=>{probe.platform=platform;});
    it('keeps original presentation through recovery without granting input',async()=>{
      const f=await fixture();keyboard();focused(true);
      fireEvent.click(screen.getByRole('button',{name:'Ctrl',exact:true}));
      const dismissed=probe.dismiss.mock.calls.length,invalidated=probe.invalidate.mock.calls.length;
      state('recovering');readonly(true);focused(true);expect(f.view.session.getIdentity()).toBeNull();
      expect(probe.dismiss).toHaveBeenCalledTimes(dismissed);expect(probe.invalidate.mock.calls.length).toBeGreaterThan(invalidated);
      expect(f.terminal).toHaveAttribute('data-modifiers','0');expect(screen.queryByText(/Snapshot since press:/)).toBeNull();
      state('recovering');state('ready');readonly(false);focused(true);
      expect(screen.getByLabelText('Native terminal')).toBe(f.terminal);
      expect(f.view.session.getIdentity()).toEqual(f.view.originalIdentity);noWork(f);
      expect(screen.queryByText(/Historical Keyboard .* ended:/)).toBeNull();
    });
    it('preserves the requested special keyboard without ever asking native focus',async()=>{
      const f=await fixture();fireEvent.click(screen.getByRole('button',{name:'Special keys',exact:true}));
      expect(screen.getByRole('button',{name:'F1',exact:true})).toBeInTheDocument();focused(false);
      state('recovering');readonly(true);focused(false);expect(screen.queryByRole('button',{name:'F1',exact:true})).toBeNull();
      state('ready');readonly(false);focused(false);expect(screen.getByRole('button',{name:'F1',exact:true})).toBeInTheDocument();noWork(f);
    });
    it('does not preserve an unvalidated ownership gap or revive it on a later ready',async()=>{
      const f=await fixture();keyboard();f.view.ownership(false);state('recovering');state('ready');readonly(true);focused(false);
      f.view.ownership(true);f.refresh();state('ready');readonly(false);focused(false);noWork(f);
    });
    it('ignores visibility changes during recovery without using a show snapshot as intent',async()=>{
      const f=await fixture();f.visibility(true);focused(false);f.visibility(false);keyboard();f.visibility(true);
      state('recovering');f.visibility(false);f.visibility(true);f.visibility(false);readonly(true);focused(true);
      state('ready');readonly(false);focused(true);noWork(f);
    });
    it('explicit queued JS HIDE wins during recovery and later visibility cannot revive it',async()=>{
      const f=await fixture();keyboard();const hide=probe.keyboardPress;expect(hide).toBeDefined();
      state('recovering');act(()=>hide?.());f.visibility(true);f.visibility(false);state('ready');
      readonly(false);focused(false);f.visibility(true);focused(false);noWork(f);
    });
    it('uses ordinary hide semantics after ready, including an indistinguishable late fence hide',async()=>{
      const f=await fixture();keyboard();f.visibility(true);state('recovering');state('ready');focused(true);
      f.visibility(false);focused(false);f.visibility(true);focused(false);noWork(f);
      expect(screen.getByText(/ended: visibility_hidden_after_ready/)).toBeInTheDocument();
    });
    for(const next of [{controlEpoch:2},{generation:'30000000-0000-4000-8000-000000000002'},
      {tabId:'20000000-0000-4000-8000-000000000002'}]){
      it(`cannot recertify SHOW on changed ownership ${Object.keys(next)[0]}`,async()=>{
        const f=await fixture();keyboard();state('recovering');f.view.owner(next,true);state('ready');readonly(false);focused(false);noWork(f);
        expect(screen.getByText(/ended: ready_context_lost/)).toBeInTheDocument();
      });
    }
    for(const next of ['blocked','closed','reconnecting'] as const){
      it(`retires desire on bridge ${next}, even if the same identity later returns`,async()=>{
        const f=await fixture();keyboard();state('recovering');f.view.bridge(next);state('ready');readonly(true);focused(false);
        f.view.bridge('attached');state('ready');readonly(false);focused(false);noWork(f);
        expect(screen.getByText(/ended: bridge_non_attached/)).toBeInTheDocument();
      });
    }
    for(const next of ['bound','closed','invalid']){
      it(`does not preserve SHOW across native ${next}`,async()=>{
        const f=await fixture();keyboard();state(next);readonly(true);focused(false);state('ready');focused(false);noWork(f);
        expect(screen.getByText(new RegExp(`ended: ${next==='invalid'?'native_readiness_invalid':'native_state_not_retained'}`))).toBeInTheDocument();
      });
    }
    it('clears desire on a matching native error and on a current session error',async()=>{
      const f=await fixture();keyboard();const native=probe.native;expect(native).toBeDefined();
      act(()=>native?.onError?.({nativeEvent:{terminalHandle:f.view.session.handle,code:5}}));
      expect(screen.getByText(/ended: native_error/)).toBeInTheDocument();
      state('ready');focused(false);keyboard();f.view.error();focused(false);noWork(f);
      expect(screen.getByText(/ended: session_error/)).toBeInTheDocument();
    });
    it('records only the first branch and resets on a new accepted action without adding core work',async()=>{
      const f=await fixture();keyboard();f.visibility(true);f.visibility(false);
      expect(screen.getByText(/ended: visibility_hidden_after_ready/)).toBeInTheDocument();const dismissals=probe.dismiss.mock.calls.length;
      act(()=>probe.native?.onError?.({nativeEvent:{terminalHandle:f.view.session.handle,code:5}}));
      expect(screen.getByText(/ended: visibility_hidden_after_ready/)).toBeInTheDocument();expect(screen.queryByText(/ended: native_error/)).toBeNull();
      state('ready');keyboard();expect(screen.queryByText(/Historical Keyboard .* ended:/)).toBeNull();
      act(()=>probe.native?.onError?.({nativeEvent:{terminalHandle:f.view.session.handle,code:5}}));
      expect(screen.getByText(/ended: native_error/)).toBeInTheDocument();
      state('ready');keyboard();f.view.error();expect(screen.getByText(/ended: session_error/)).toBeInTheDocument();
      expect(probe.dismiss).toHaveBeenCalledTimes(dismissals+3);noWork(f);
    });
    it('records selection close with the existing dismissal and without input or view replacement',async()=>{
      const f=await fixture();keyboard();const dismissals=probe.dismiss.mock.calls.length;
      fireEvent.click(screen.getByRole('button',{name:'Select',exact:true}));
      expect(screen.getByText(/ended: selection_close/)).toBeInTheDocument();readonly(false);focused(false);
      expect(probe.dismiss).toHaveBeenCalledTimes(dismissals+1);noWork(f);
    });
    it('leaves no record before accepted action and a blocked or destroyed destination unavailable',async()=>{
      const f=await fixture();state('invalid');expect(screen.queryByText(/Historical Keyboard .* ended:/)).toBeNull();
      state('ready');keyboard();f.view.bridge('blocked');
      expect(screen.queryByText(/Historical Keyboard .* ended:/)).toBeNull();
      expect(screen.getByText('Another client is controlling this terminal')).toBeInTheDocument();
      f.unmount();expect(screen.queryByText(/Historical Keyboard .* ended:/)).toBeNull();expect(f.view.session.close).toHaveBeenCalledTimes(1);
    });
    it('cannot revive old desire through stale native readiness or visibility on a replacement view',async()=>{
      const f=await fixture();keyboard();const old=probe.native;
      await f.install({connectionEpoch:'replacement'});state('ready');readonly(false);focused(false);
      expect(screen.queryByText(/Historical Keyboard .* ended:/)).toBeNull();expect(screen.queryByText(/Keyboard SHOW\. Snapshot/)).toBeNull();
      act(()=>old?.onState?.({nativeEvent:{terminalHandle:f.view.session.handle,state:'ready'}}));
      f.visibility(true);focused(false);expect(f.host.driver.open).toHaveBeenCalledTimes(2);
      expect(f.view.session.close).toHaveBeenCalledTimes(1);expect(f.view.session.input).not.toHaveBeenCalled();
    });
    it('does not retarget a queued SHOW to a replacement epoch or ownership identity',async()=>{
      const f=await fixture();const oldEpochShow=probe.keyboardPress;expect(oldEpochShow).toBeDefined();
      await f.install({connectionEpoch:'new-epoch'});state('ready');act(()=>oldEpochShow?.());focused(false);
      const oldOwnerShow=probe.keyboardPress;expect(oldOwnerShow).toBeDefined();
      const current=f.host.views.at(-1);expect(current).toBeDefined();
      current?.owner({controlEpoch:2},true);f.refresh();act(()=>oldOwnerShow?.());focused(false);
      keyboard();focused(true);state('recovering');state('ready');focused(true);
      for(const view of f.host.views)expect(view.session.input).not.toHaveBeenCalled();
    });
    it('does not let queued SHOW or HIDE act on a newer requested context',async()=>{
      const f=await fixture();const oldShow=probe.keyboardPress;keyboard();const oldHide=probe.keyboardPress;
      await f.install({connectionEpoch:'new-context'});state('ready');keyboard();focused(true);
      const notice=screen.getByText('Keyboard SHOW. Snapshot since press: notObserved.').textContent;
      act(()=>{oldShow?.();oldHide?.();});focused(true);readonly(false);
      expect(screen.getByText('Keyboard SHOW. Snapshot since press: notObserved.').textContent).toBe(notice);
      expect(screen.queryByText(/Historical Keyboard .* ended:/)).toBeNull();
      const current=f.host.views.at(-1);expect(current).toBeDefined();const sameViewOldHide=probe.keyboardPress;
      current?.owner({controlEpoch:2},true);f.refresh();keyboard();focused(true);
      act(()=>sameViewOldHide?.());focused(true);f.visibility(true);keyboard();focused(false);
      for(const view of f.host.views)expect(view.session.input).not.toHaveBeenCalled();
    });
    it('ignores a queued local presentation callback from the retired original view',async()=>{
      const f=await fixture();keyboard();const old=f.view.stateCallback();expect(old).toBeDefined();
      await f.install({connectionEpoch:'replacement'});state('ready');keyboard();focused(true);
      act(()=>old?.('reconnecting',{kind:'layoutRecovery',phase:'pending',identity:f.view.originalIdentity}));
      readonly(false);focused(true);expect(f.host.driver.open).toHaveBeenCalledTimes(2);
      expect(f.view.session.close).toHaveBeenCalledTimes(1);for(const view of f.host.views)expect(view.session.input).not.toHaveBeenCalled();
    });
    it('clears across driver, tab, readonly and unmount/remount lifecycle changes',async()=>{
      const f=await fixture();keyboard();const replacement=makeDriver();await f.install({driver:replacement.driver});state('ready');focused(false);
      keyboard();await f.install({tabId:'20000000-0000-4000-8000-000000000002'});state('ready');focused(false);
      keyboard();await f.install({readOnly:true});state('ready');readonly(true);focused(false);
      await f.install({readOnly:false});state('ready');readonly(false);focused(false);
      f.unmount();await fixture();focused(false);
      for(const view of [...f.host.views,...replacement.views])expect(view.session.input).not.toHaveBeenCalled();
    });
  });
}
function pressed(name:string){return screen.getByRole('button',{name,exact:true}).getAttribute('aria-pressed')==='true';}
function special(){fireEvent.click(screen.getByRole('button',{name:'Special keys',exact:true}));}
function surfaceTap(){
  const native=probe.native;if(!native)throw new Error('Native view missing');
  const focusResult={platform:'android',source:'surface',attached:true,windowFocused:true,uiThread:true,inputAllowed:true,
    frameAvailable:true,focusRequest:'accepted',focused:true,keyboardRequest:'accepted'};
  act(()=>native.onState?.({nativeEvent:{terminalHandle:native.terminalHandle,state:'focusResult',focusResult} as never}));
}
describe('key bar toggles follow what is on screen (0.9.15 rules)',()=>{
  beforeEach(()=>{probe.platform='android';});
  it('closes the Fn row when the IME appears and never shows both active',async()=>{
    const f=await fixture();special();expect(pressed('Special keys')).toBe(true);expect(pressed('Keyboard')).toBe(false);
    f.visibility(true);
    expect(pressed('Keyboard')).toBe(true);expect(pressed('Special keys')).toBe(false);
    f.visibility(false);expect(pressed('Special keys')).toBe(false);expect(pressed('Keyboard')).toBe(false);
  });
  it('shows the keyboard toggle inactive when the OS hid the IME, and a tap opens it again',async()=>{
    const f=await fixture();keyboard();focused(true);expect(pressed('Keyboard')).toBe(false);
    // The IME never appeared (hardware keyboard, or hidden before ready): request focus again.
    probe.focusLog.length=0;keyboard();focused(true);expect(probe.focusLog).toContain(false);expect(probe.dismiss).toHaveBeenCalled();
    f.visibility(true);expect(pressed('Keyboard')).toBe(true);
    keyboard();focused(false);expect(pressed('Keyboard')).toBe(true);
    f.visibility(false);expect(pressed('Keyboard')).toBe(false);
  });
  it('treats a tap on the terminal as a keyboard request so the toggle can hide it',async()=>{
    const f=await fixture();special();surfaceTap();focused(true);
    f.visibility(true);expect(pressed('Keyboard')).toBe(true);expect(pressed('Special keys')).toBe(false);
    keyboard();focused(false);
  });
});
