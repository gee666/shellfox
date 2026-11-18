// @vitest-environment jsdom
import {act,screen} from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import {beforeEach,describe,expect,it} from 'vitest';
import {captureLayoutPresentation,validateLayoutPresentation} from './layout-presentation';
import type {LayoutComparisonOutcome} from '../../terminal/keyboard-retirement';
import {attachmentSchema,type ManagerAttachment} from './schemas';
import {probe,original,owned,deferred,tick,setup,state,keyboard,layout,focus,fence,sameInstallation} from './layout-test-fixture';

describe('C metadata observes actual comparisons without changing short-circuit evaluation',()=>{
  function marker(){const captured=captureLayoutPresentation(owned,owned);if(!captured)throw Error('Original fixture marker missing');return captured;}
  it('leaves validated comparisons silent and cannot create continuity from absent before-context',()=>{
    const outcomes:LayoutComparisonOutcome[]=[];
    expect(validateLayoutPresentation(marker(),owned,value=>outcomes.push(value))).toBe(true);expect(outcomes).toEqual([]);
    expect(captureLayoutPresentation(owned,undefined)).toBeUndefined();
  });
  it('stops before reading the reply when an original marker has retired',()=>{
    const captured=marker(),reply=attachmentSchema.parse(owned),outcomes:LayoutComparisonOutcome[]=[];captured.valid=false;
    Object.defineProperty(reply,'kind',{get(){throw Error('Retired comparison must not read reply');}});
    expect(validateLayoutPresentation(captured,reply,value=>outcomes.push(value))).toBe(false);expect(outcomes).toEqual(['intervening_retirement']);
  });
  it('stops before delivery or control comparisons when the actual response is not owned',()=>{
    const reply=attachmentSchema.parse({...owned,input:null,inputState:'busy'}),outcomes:LayoutComparisonOutcome[]=[];
    Object.defineProperty(reply,'delivery',{get(){throw Error('Non-owned reply must not read delivery');}});
    expect(validateLayoutPresentation(marker(),reply,value=>outcomes.push(value))).toBe(false);expect(outcomes).toEqual(['response_not_owned']);
  });
  it('classifies a full-control failure at that comparison and never reads a later delivery predicate',()=>{
    const reply=attachmentSchema.parse({...owned,input:{...original.input,controlId:'a0000000-0000-4000-8000-000000000002'}}),outcomes:LayoutComparisonOutcome[]=[];
    Object.defineProperty(reply,'delivery',{get(){throw Error('Failed control comparison must short-circuit');}});
    expect(validateLayoutPresentation(marker(),reply,value=>outcomes.push(value))).toBe(false);expect(outcomes).toEqual(['response_control_changed']);
  });
  it('preserves field-level control short-circuit order',()=>{
    const source=attachmentSchema.parse(owned);if(source.kind!=='attached'||!source.input)throw Error('Owned fixture missing');
    // Direct inconsistent structure tests the predicate, not schema admission or wire authority.
    const reply={...source,input:{...source.input,tabId:'50000000-0000-4000-8000-000000000009'}},outcomes:LayoutComparisonOutcome[]=[];
    Object.defineProperty(reply.input,'controlEpoch',{get(){throw Error('First control field mismatch must short-circuit');}});
    expect(validateLayoutPresentation(marker(),reply,value=>outcomes.push(value))).toBe(false);expect(outcomes).toEqual(['response_control_changed']);
  });
  it.each(['tabId','generation'] as const)('labels the defensive delivery %s comparison only when it runs',field=>{
    const source=attachmentSchema.parse(owned);if(source.kind!=='attached')throw Error('Attached fixture missing');
    // Deliberately inconsistent local structure for the defensive predicate, not parsed wire authority.
    const reply:ManagerAttachment={...source,delivery:{...source.delivery,[field]:'50000000-0000-4000-8000-000000000009'}};
    const outcomes:LayoutComparisonOutcome[]=[];
    expect(validateLayoutPresentation(marker(),reply,value=>outcomes.push(value))).toBe(false);expect(outcomes).toEqual(['response_generation_changed']);
  });
});
for(const platform of ['android','ios'] as const){
 describe(`C real ManagerSurface layout route ${platform}, no native authority`,()=>{
  beforeEach(()=>{probe.platform=platform;});
  it('keeps only causal layout presentation in the same original surface and binding',async()=>{
    const f=await setup();keyboard();const reply=deferred<ManagerAttachment>();f.pending(reply.promise);
    act(()=>f.surface.event(layout()));await tick();
    expect(f.surface.getState()).toBe('reconnecting');expect(f.surface.getIdentity()).toBeNull();fence(true);focus(true);sameInstallation(f);
    reply.resolve(owned);await tick(250);state('ready');
    expect(f.surface.getState()).toBe('attached');expect(f.surface.getIdentity()).toEqual(f.identity);
    expect(screen.getByLabelText('Native terminal')).toBe(f.terminal);fence(false);focus(true);sameInstallation(f);
    expect(f.native.calls.filter(v=>v==='attachTerminal')).toHaveLength(2);
    expect(screen.queryByText(/Historical Keyboard .* ended:/)).toBeNull();
  });
  it('restores captured layout desire only after original-owner response and genuine ready',async()=>{
    const f=await setup();keyboard();const reply=deferred<ManagerAttachment>();f.pending(reply.promise);
    act(()=>f.surface.event(layout()));await tick();fence(true);expect(f.surface.getIdentity()).toBeNull();sameInstallation(f);
    reply.resolve(owned);await tick(250);state('ready');
    expect(f.surface.getIdentity()).toEqual(f.identity);expect(screen.getByLabelText('Native terminal')).toBe(f.terminal);fence(false);sameInstallation(f);
    focus(true);
  });
  it('GREEN ignores non-current delivery and layouts that require no recovery',async()=>{
    const f=await setup();keyboard();act(()=>{f.surface.event(layout({...original.delivery,deliveryEpoch:8}));f.surface.event(layout(original.delivery,false));});await tick();
    focus(true);fence(false);sameInstallation(f);expect(f.native.calls.filter(v=>v==='attachTerminal')).toHaveLength(1);
  });
  it('GREEN changed response owner cannot recertify the original desire',async()=>{
    const f=await setup();keyboard();const reply=deferred<ManagerAttachment>();f.pending(reply.promise);act(()=>f.surface.event(layout()));await tick();
    reply.resolve(attachmentSchema.parse({...owned,input:{...original.input,controlEpoch:8}}));await tick(250);state('ready');
    expect(f.surface.getIdentity()).not.toEqual(f.identity);fence(false);focus(false);sameInstallation(f);
  });
  it('GREEN a different control reference cannot revive desire even when the UI identity triple is equal',async()=>{
    const f=await setup();keyboard();const reply=deferred<ManagerAttachment>();f.pending(reply.promise);act(()=>f.surface.event(layout()));await tick();
    reply.resolve(attachmentSchema.parse({...owned,input:{...original.input,controlId:'a0000000-0000-4000-8000-000000000002'}}));await tick(250);state('ready');
    expect(f.surface.getIdentity()).toEqual(f.identity);focus(false);sameInstallation(f);
    expect(screen.getByText(/ended: layout_validation_lost \/ response_control_changed/)).toBeInTheDocument();
  });
  it('rejects saved owner-A SHOW after full control retirement even when the UI triple still matches',async()=>{
    const f=await setup();const oldShow=probe.keyboardPress;expect(oldShow).toBeDefined();keyboard();
    const reply=deferred<ManagerAttachment>();f.pending(reply.promise);act(()=>f.surface.event(layout()));await tick();
    reply.resolve(attachmentSchema.parse({...owned,input:{...original.input,controlId:'a0000000-0000-4000-8000-000000000002'}}));await tick(250);state('ready');
    expect(f.surface.getIdentity()).toEqual(f.identity);fence(false);focus(false);sameInstallation(f);
    const operations=f.native.calls.length,configures=f.native.configureTerminal.mock.calls.length;
    const notice=screen.getByText(/Historical Keyboard SHOW ended:/).textContent;
    act(()=>oldShow?.());sameInstallation(f);expect(f.native.calls).toHaveLength(operations);expect(f.native.configureTerminal).toHaveBeenCalledTimes(configures);focus(false);
    expect(screen.getByText(/Historical Keyboard SHOW ended:/).textContent).toBe(notice);
    keyboard();focus(true);sameInstallation(f);
  });
  it('rejects saved owner-A HIDE without erasing a fresh owner-B intent with an equal UI triple',async()=>{
    const f=await setup();keyboard();const oldHide=probe.keyboardPress;expect(oldHide).toBeDefined();
    const reply=deferred<ManagerAttachment>();f.pending(reply.promise);act(()=>f.surface.event(layout()));await tick();
    reply.resolve(attachmentSchema.parse({...owned,input:{...original.input,controlId:'a0000000-0000-4000-8000-000000000002'}}));await tick(250);state('ready');
    expect(f.surface.getIdentity()).toEqual(f.identity);fence(false);focus(false);keyboard();focus(true);
    const operations=f.native.calls.length,configures=f.native.configureTerminal.mock.calls.length;
    const notice=screen.getByText('Keyboard SHOW. Snapshot since press: notObserved.').textContent;
    act(()=>oldHide?.());sameInstallation(f);expect(f.native.calls).toHaveLength(operations);expect(f.native.configureTerminal).toHaveBeenCalledTimes(configures);focus(true);
    expect(screen.getByText('Keyboard SHOW. Snapshot since press: notObserved.').textContent).toBe(notice);expect(screen.queryByText(/Historical Keyboard .* ended:/)).toBeNull();
    keyboard();focus(false);sameInstallation(f);
  });
  it('GREEN a busy response does not create input authority or restore desire',async()=>{
    const f=await setup();keyboard();const reply=deferred<ManagerAttachment>();f.pending(reply.promise);act(()=>f.surface.event(layout()));await tick();
    reply.resolve(attachmentSchema.parse({...owned,input:null,inputState:'busy'}));await tick(250);state('ready');
    expect(f.surface.getIdentity()).toBeNull();expect(f.surface.getState()).toBe('blocked');fence(true);focus(false);sameInstallation(f);
  });
  it('GREEN attachment failure keeps input unavailable and clears desire',async()=>{
    const f=await setup();keyboard();const reply=deferred<ManagerAttachment>();f.pending(reply.promise);act(()=>f.surface.event(layout()));await tick();
    reply.reject(new Error('Inert layout attachment failed'));await tick(250);state('ready');fence(true);focus(false);expect(f.surface.getIdentity()).toBeNull();sameInstallation(f);
    expect(screen.getByText(/ended: bridge_non_attached \/ request_failed_or_unknown/)).toBeInTheDocument();
  });
  it('GREEN changed connection binding on the layout response is rejected, not recertified',async()=>{
    const f=await setup();keyboard();const reply=deferred<ManagerAttachment>();f.pending(reply.promise);act(()=>f.surface.event(layout()));await tick();
    if(!f.binding)throw Error('Original binding missing');
    f.differentReplyConnection({status:'connected',binding:{...f.binding,connectionId:'40000000-0000-4000-8000-000000000002'}});
    reply.resolve(owned);await tick(250);state('ready');expect(f.surface.getIdentity()).toBeNull();fence(true);focus(false);
    expect(f.native.pairManagerV2).toHaveBeenCalledTimes(1);expect(f.native.bindTerminal).toHaveBeenCalledTimes(1);expect(f.native.terminalInput).not.toHaveBeenCalled();
  });
  it('allows a checked fresh delivery only for the same original full control and binding',async()=>{
    const f=await setup();keyboard();const nextDelivery={...original.delivery,deliveryId:'70000000-0000-4000-8000-000000000002',deliveryEpoch:8};
    const reply=deferred<ManagerAttachment>();f.pending(reply.promise);act(()=>f.surface.event(layout()));await tick();fence(true);focus(true);
    reply.resolve(attachmentSchema.parse({...owned,delivery:nextDelivery}));await tick(250);state('ready');
    expect(f.surface.getIdentity()).toEqual(f.identity);fence(false);focus(true);sameInstallation(f);
    act(()=>f.surface.event(layout()));await tick();expect(f.native.calls.filter(v=>v==='attachTerminal')).toHaveLength(2);
    const again=deferred<ManagerAttachment>();f.pending(again.promise);act(()=>f.surface.event(layout(nextDelivery)));await tick();fence(true);focus(true);
    again.resolve(attachmentSchema.parse({...owned,delivery:{...nextDelivery,deliveryEpoch:9}}));await tick(250);state('ready');
    fence(false);focus(true);sameInstallation(f);expect(f.native.calls.filter(v=>v==='attachTerminal')).toHaveLength(3);
  });
  it('a repeated matching layout event joins the original layout operation without another attachment',async()=>{
    const f=await setup();keyboard();const reply=deferred<ManagerAttachment>();f.pending(reply.promise);
    act(()=>{f.surface.event(layout());f.surface.event(layout());});await tick();fence(true);focus(true);
    expect(f.native.calls.filter(v=>v==='attachTerminal')).toHaveLength(2);
    reply.resolve(owned);await tick(250);state('ready');focus(true);sameInstallation(f);
  });
  it('does not upgrade an unrelated pending attachment or issue another request',async()=>{
    const f=await setup();keyboard();const reply=deferred<ManagerAttachment>();f.pending(reply.promise);
    const original=f.surface.reattach();act(()=>f.surface.event(layout()));await tick();fence(true);focus(false);
    expect(f.native.calls.filter(v=>v==='attachTerminal')).toHaveLength(2);
    reply.resolve(owned);await tick(250);await original;state('ready');fence(false);focus(false);sameInstallation(f);
    expect(screen.getByText(/ended: bridge_non_attached/)).toBeInTheDocument();expect(screen.queryByText(/pending_unrelated/)).toBeNull();
  });
  it('retains an unknown original attach and close promise without replay or destructive cleanup',async()=>{
    const f=await setup();keyboard();const reply=deferred<ManagerAttachment>();f.pending(reply.promise);act(()=>f.surface.event(layout()));await tick();
    reply.reject(new Error('Unknown original attach outcome'));await tick(250);
    const held=f.surface.reattach();expect(f.surface.reattach()).toBe(held);await expect(held).rejects.toThrow('Unknown original');
    act(()=>f.surface.event(layout()));await tick();expect(f.native.calls.filter(v=>v==='attachTerminal')).toHaveLength(2);
    const closing=f.surface.close();expect(f.surface.close()).toBe(closing);await expect(closing).rejects.toThrow('Unknown original');
    expect(f.native.unbindTerminal).not.toHaveBeenCalled();expect(f.native.closeTerminal).not.toHaveBeenCalled();expect(f.native.reconnectManagerV2).not.toHaveBeenCalled();
    fence(true);focus(false);
  });
  it('keeps input blocked if native ready precedes the original checked layout result',async()=>{
    const f=await setup();keyboard();const reply=deferred<ManagerAttachment>();f.pending(reply.promise);act(()=>f.surface.event(layout()));await tick();
    state('ready');expect(f.surface.getIdentity()).toBeNull();fence(true);focus(true);
    reply.resolve(owned);await tick(250);fence(false);focus(true);expect(f.surface.getIdentity()).toEqual(f.identity);sameInstallation(f);
  });
  it('never treats a retired local notice as validated-ready proof for a later action',async()=>{
    const f=await setup();keyboard();const reply=deferred<ManagerAttachment>();f.pending(reply.promise);act(()=>f.surface.event(layout()));await tick();state('ready');
    reply.resolve(attachmentSchema.parse({...owned,input:{...original.input,controlId:'a0000000-0000-4000-8000-000000000002'}}));await tick(250);
    fence(false);focus(false);keyboard();focus(true);act(()=>f.surface.event(layout()));await tick(250);state('ready');
    focus(false);expect(screen.getByText(/ended: bridge_non_attached \/ before_context_unavailable/)).toBeInTheDocument();sameInstallation(f);
  });
  it('explicit queued HIDE wins while a matching layout operation remains pending',async()=>{
    const f=await setup();keyboard();const hide=probe.keyboardPress;expect(hide).toBeDefined();
    const reply=deferred<ManagerAttachment>();f.pending(reply.promise);act(()=>f.surface.event(layout()));await tick();act(()=>hide?.());
    reply.resolve(owned);await tick(250);state('ready');fence(false);focus(false);sameInstallation(f);
  });
  it('a matching native error retires layout desire before its checked response',async()=>{
    const f=await setup();keyboard();const originalNative=probe.native;
    const reply=deferred<ManagerAttachment>();f.pending(reply.promise);act(()=>f.surface.event(layout()));await tick();
    act(()=>originalNative?.onError?.({nativeEvent:{terminalHandle:f.surface.handle,code:5}}));
    reply.resolve(owned);await tick(250);state('ready');focus(false);sameInstallation(f);
    expect(screen.getByText(/ended: native_error/)).toBeInTheDocument();expect(screen.queryByText(/intervening_retirement/)).toBeNull();
  });
  it('classifies unavailable before-context only at the existing fallback branch',async()=>{
    const f=await setup();keyboard();act(()=>{f.surface.nativeState('bound');f.surface.event(layout());});await tick(250);state('ready');
    expect(screen.getByText(/ended: bridge_non_attached \/ before_context_unavailable/)).toBeInTheDocument();focus(false);sameInstallation(f);
  });
  it('does not create a retirement record merely because a layout reply comparison ran',async()=>{
    const f=await setup();const reply=deferred<ManagerAttachment>();f.pending(reply.promise);act(()=>f.surface.event(layout()));await tick();
    reply.resolve(attachmentSchema.parse({...owned,input:{...original.input,controlId:'a0000000-0000-4000-8000-000000000002'}}));await tick(250);state('ready');
    expect(screen.queryByText(/Historical Keyboard .* ended:/)).toBeNull();sameInstallation(f);
  });
  it('registers the original layout operation before a presentation listener closes the surface',async()=>{
    const f=await setup();keyboard();const reply=deferred<ManagerAttachment>();f.pending(reply.promise);
    let closing:Promise<void>|undefined;
    const unsubscribe=f.surface.subscribe((_state,notice)=>{if(notice?.phase==='pending')closing=f.surface.close();});
    act(()=>f.surface.event(layout()));await tick();expect(closing).toBeDefined();
    expect(f.native.unbindTerminal).not.toHaveBeenCalled();expect(f.native.closeTerminal).not.toHaveBeenCalled();
    reply.resolve(owned);await tick(400);await closing;unsubscribe();f.refresh();state('ready');focus(false);
    expect(f.native.calls.filter(v=>v==='attachTerminal')).toHaveLength(2);expect(f.native.closeTerminal).toHaveBeenCalledTimes(1);
  });
  it('joins the registered original client close under a reentrant presentation callback',async()=>{
    const f=await setup();keyboard();let nested:Promise<void>|undefined;
    const unsubscribe=f.surface.subscribe(()=>{nested=f.client.close();});
    const closing=f.client.close();expect(nested).toBe(closing);await tick(400);await closing;unsubscribe();
    expect(f.native.unbindTerminal).toHaveBeenCalledTimes(1);expect(f.native.closeTerminal).toHaveBeenCalledTimes(1);expect(f.native.terminalInput).not.toHaveBeenCalled();
  });
  it('GREEN revoked delivery is not a benign layout window',async()=>{
    const f=await setup();keyboard();act(()=>f.surface.event({type:'terminal',event:{type:'revoked',delivery:original.delivery,reason:'approvalRevoked'}}));await tick();state('ready');
    fence(true);focus(false);expect(f.surface.getIdentity()).toBeNull();expect(f.native.calls.filter(v=>v==='attachTerminal')).toHaveLength(1);
  });
  it('GREEN intervening revocation cannot restore old desire from a same-identity pending response',async()=>{
    const f=await setup();keyboard();const reply=deferred<ManagerAttachment>();f.pending(reply.promise);act(()=>f.surface.event(layout()));await tick();
    act(()=>f.surface.event({type:'terminal',event:{type:'revoked',delivery:original.delivery,reason:'approvalRevoked'}}));await tick();
    reply.resolve(owned);await tick(250);state('ready');focus(false);expect(f.native.terminalInput).not.toHaveBeenCalled();
  });
  it('GREEN disconnect during a layout wait cannot restore a closing original surface',async()=>{
    const f=await setup();keyboard();const reply=deferred<ManagerAttachment>();f.pending(reply.promise);act(()=>f.surface.event(layout()));await tick();
    const retiring=f.client.disconnect();await tick();expect(f.native.disconnectManagerV2).not.toHaveBeenCalled();
    reply.resolve(owned);await tick(400);await retiring;f.refresh();state('ready');
    expect(f.client.getState().status).toBe('disconnected');expect(f.surface.getIdentity()).toBeNull();fence(true);focus(false);expect(f.native.terminalInput).not.toHaveBeenCalled();
  });
  it('GREEN real disconnect retires the original surface before native disconnect',async()=>{
    const f=await setup();keyboard();const disconnect=f.client.disconnect();await tick(350);await disconnect;f.refresh();state('ready');
    expect(f.client.getState().status).toBe('disconnected');expect(f.surface.getIdentity()).toBeNull();fence(true);focus(false);
    expect(f.native.closeTerminal).toHaveBeenCalledTimes(1);expect(f.native.unbindTerminal).toHaveBeenCalledTimes(1);expect(f.native.terminalInput).not.toHaveBeenCalled();
  });
 });
}

