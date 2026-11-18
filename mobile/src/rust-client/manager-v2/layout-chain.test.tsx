// @vitest-environment jsdom
import {act,screen} from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import {beforeEach,describe,expect,it} from 'vitest';
import {attachmentSchema,type ManagerAttachment} from './schemas';
import {probe,original,owned,deferred,tick,setup,state,keyboard,layout,focus,fence,sameInstallation} from './layout-test-fixture';

for(const platform of ['android','ios'] as const){
 describe(`checked original layout-chain presentation ${platform}`,()=>{
  beforeEach(()=>{probe.platform=platform;});
  it('original native recovering before one causal layout does not by itself lose continuity',async()=>{
    const f=await setup();keyboard();state('recovering');focus(true);fence(true);
    const reply=deferred<ManagerAttachment>();f.pending(reply.promise);act(()=>f.surface.event(layout()));await tick();
    focus(true);fence(true);expect(f.surface.getIdentity()).toBeNull();reply.resolve(owned);await tick(250);state('ready');
    focus(true);fence(false);sameInstallation(f);expect(f.native.calls.filter(v=>v==='attachTerminal')).toHaveLength(2);
  });
  it('genuine native ready between consecutive original layouts restores the required before proof',async()=>{
    const f=await setup();keyboard();const a=deferred<ManagerAttachment>();f.pending(a.promise);act(()=>f.surface.event(layout()));await tick();
    a.resolve(owned);await tick(250);state('ready');focus(true);
    const b=deferred<ManagerAttachment>();f.pending(b.promise);act(()=>f.surface.event(layout(original.delivery,true,3)));await tick();
    focus(true);fence(true);expect(f.surface.getIdentity()).toBeNull();b.resolve(owned);await tick(250);state('ready');
    focus(true);fence(false);sameInstallation(f);expect(f.native.calls.filter(v=>v==='attachTerminal')).toHaveLength(3);
  });
  for(const freshDelivery of [false,true])it(`retains original checked recovery desire across another causal layout BEFORE native ready, freshDelivery=${freshDelivery}`,async()=>{
    const f=await setup();keyboard();const delivery=freshDelivery?{...original.delivery,deliveryId:'70000000-0000-4000-8000-000000000002',deliveryEpoch:8}:original.delivery;
    const response=attachmentSchema.parse({...owned,delivery}),a=deferred<ManagerAttachment>();f.pending(a.promise);act(()=>f.surface.event(layout()));await tick();
    a.resolve(response);await tick(250);focus(true);fence(true);expect(f.surface.getIdentity()).toBeNull();
    const b=deferred<ManagerAttachment>();f.pending(b.promise);act(()=>f.surface.event(layout(delivery,true,3)));await tick();
    const during=screen.getByLabelText('Native terminal').getAttribute('data-focused'),readOnlyDuring=screen.getByLabelText('Native terminal').getAttribute('data-readonly');
    expect(f.surface.getIdentity()).toBeNull();
    // Settle both original operations before checking presentation.
    b.resolve(response);await tick(250);state('ready');const after=screen.getByLabelText('Native terminal').getAttribute('data-focused');
    expect(f.surface.getIdentity()).toEqual(f.identity);fence(false);sameInstallation(f);
    expect(f.native.calls.filter(v=>v==='attachTerminal')).toHaveLength(3);
    expect({during,after,readOnlyDuring}).toEqual({during:'true',after:'true',readOnlyDuring:'true'});
  });
  it('explicit HIDE and its revision fence survive a checked chain without reviving saved callbacks',async()=>{
    const f=await setup();const oldShow=probe.keyboardPress;keyboard();const hide=probe.keyboardPress;
    const a=deferred<ManagerAttachment>();f.pending(a.promise);act(()=>f.surface.event(layout()));await tick();act(()=>hide?.());
    a.resolve(owned);await tick(250);focus(false);fence(true);
    const b=deferred<ManagerAttachment>();f.pending(b.promise);act(()=>f.surface.event(layout(original.delivery,true,3)));await tick();b.resolve(owned);await tick(250);state('ready');
    act(()=>oldShow?.());focus(false);keyboard();focus(true);act(()=>hide?.());focus(true);sameInstallation(f);
    expect(f.native.calls.filter(v=>v==='attachTerminal')).toHaveLength(3);
  });
  it('ignores the old delivery while an original checked fresh-delivery chain still awaits native ready',async()=>{
    const f=await setup();keyboard();const delivery={...original.delivery,deliveryId:'70000000-0000-4000-8000-000000000002',deliveryEpoch:8};
    const response=attachmentSchema.parse({...owned,delivery}),a=deferred<ManagerAttachment>();f.pending(a.promise);act(()=>f.surface.event(layout()));await tick();a.resolve(response);await tick(250);
    act(()=>f.surface.event(layout(original.delivery,true,3)));await tick();focus(true);fence(true);expect(f.native.calls.filter(v=>v==='attachTerminal')).toHaveLength(2);
    const b=deferred<ManagerAttachment>();f.pending(b.promise);act(()=>f.surface.event(layout(delivery,true,4)));await tick();focus(true);fence(true);expect(f.surface.getIdentity()).toBeNull();
    b.resolve(response);await tick(250);state('ready');focus(true);fence(false);sameInstallation(f);expect(f.native.calls.filter(v=>v==='attachTerminal')).toHaveLength(3);
  });
  it('does not upgrade a generic pending attach after the checked result while native ready is still absent',async()=>{
    const f=await setup();keyboard();const a=deferred<ManagerAttachment>();f.pending(a.promise);act(()=>f.surface.event(layout()));await tick();a.resolve(owned);await tick(250);focus(true);fence(true);
    const b=deferred<ManagerAttachment>();f.pending(b.promise);const held=f.surface.reattach();act(()=>f.surface.event(layout(original.delivery,true,3)));await tick();focus(false);fence(true);
    b.resolve(owned);await tick(250);await held;state('ready');focus(false);fence(false);sameInstallation(f);expect(f.native.calls.filter(v=>v==='attachTerminal')).toHaveLength(3);
  });
  it('keeps the original operation registered when a validated-result listener closes the surface',async()=>{
    const f=await setup();keyboard();const reply=deferred<ManagerAttachment>();f.pending(reply.promise);act(()=>f.surface.event(layout()));await tick();
    let closing:Promise<void>|undefined;const unsubscribe=f.surface.subscribe((_state,notice)=>{if(notice?.phase==='validated')closing=f.surface.close();});
    reply.resolve(owned);await tick(400);expect(closing).toBeDefined();await closing;unsubscribe();state('ready');focus(false);fence(true);
    expect(f.surface.getIdentity()).toBeNull();expect(f.native.unbindTerminal).toHaveBeenCalledTimes(1);expect(f.native.closeTerminal).toHaveBeenCalledTimes(1);
    expect(f.native.calls.filter(v=>v==='attachTerminal')).toHaveLength(2);expect(f.native.pairManagerV2).toHaveBeenCalledTimes(1);expect(f.native.terminalInput).not.toHaveBeenCalled();expect(f.native.pollManagerV2).not.toHaveBeenCalled();expect(f.native.progress).not.toHaveBeenCalled();
  });
  it('unrelated generic reconnect is destructive and cannot be upgraded by a matching layout',async()=>{
    const f=await setup();keyboard();const reply=deferred<ManagerAttachment>();f.pending(reply.promise);
    const held=f.surface.reattach();act(()=>f.surface.event(layout()));await tick();reply.resolve(owned);await tick(250);await held;state('ready');
    focus(false);fence(false);sameInstallation(f);expect(f.native.calls.filter(v=>v==='attachTerminal')).toHaveLength(2);
  });
  it('bound is not benign recovering and does not create a replacement before proof',async()=>{
    const f=await setup();keyboard();state('bound');const reply=deferred<ManagerAttachment>();f.pending(reply.promise);
    act(()=>f.surface.event(layout()));await tick();reply.resolve(owned);await tick(250);state('ready');
    focus(false);sameInstallation(f);expect(screen.getByText(/ended: native_state_not_retained/)).toBeInTheDocument();
  });
  it('changed full control cannot retain desire even with an equal UI identity triple',async()=>{
    const f=await setup();keyboard();const reply=deferred<ManagerAttachment>();f.pending(reply.promise);act(()=>f.surface.event(layout()));await tick();
    reply.resolve(attachmentSchema.parse({...owned,input:{...original.input,controlId:'a0000000-0000-4000-8000-000000000002'}}));await tick(250);state('ready');
    expect(f.surface.getIdentity()).toEqual(f.identity);focus(false);sameInstallation(f);
    expect(screen.getByText(/ended: layout_validation_lost \/ response_control_changed/)).toBeInTheDocument();
  });
  it('changed binding rejects the original result and cannot restore its desire',async()=>{
    const f=await setup();keyboard();const reply=deferred<ManagerAttachment>();f.pending(reply.promise);act(()=>f.surface.event(layout()));await tick();
    if(!f.binding)throw Error('Original binding missing');f.differentReplyConnection({status:'connected',binding:{...f.binding,connectionId:'40000000-0000-4000-8000-000000000002'}});
    reply.resolve(owned);await tick(250);state('ready');focus(false);fence(true);expect(f.surface.getIdentity()).toBeNull();
    expect(f.native.terminalInput).not.toHaveBeenCalled();expect(f.native.pairManagerV2).toHaveBeenCalledTimes(1);
  });
  it('failed/unknown original attach remains sticky and cannot be replayed for a before proof',async()=>{
    const f=await setup();keyboard();const reply=deferred<ManagerAttachment>();f.pending(reply.promise);act(()=>f.surface.event(layout()));await tick();
    reply.reject(new Error('Unknown original inert attachment result'));await tick(250);const held=f.surface.reattach();
    expect(f.surface.reattach()).toBe(held);await expect(held).rejects.toThrow('Unknown original inert');act(()=>f.surface.event(layout()));await tick();state('ready');
    focus(false);fence(true);sameInstallation(f);expect(f.native.calls.filter(v=>v==='attachTerminal')).toHaveLength(2);
  });
  it('revocation between checked result and native ready retires original intent',async()=>{
    const f=await setup();keyboard();const reply=deferred<ManagerAttachment>();f.pending(reply.promise);act(()=>f.surface.event(layout()));await tick();
    reply.resolve(owned);await tick(250);act(()=>f.surface.event({type:'terminal',event:{type:'revoked',delivery:original.delivery,reason:'approvalRevoked'}}));state('ready');
    focus(false);fence(true);expect(f.surface.getIdentity()).toBeNull();sameInstallation(f);
  });
  it('non-owned response does not retain keyboard intent or create input authority',async()=>{
    const f=await setup();keyboard();const reply=deferred<ManagerAttachment>();f.pending(reply.promise);act(()=>f.surface.event(layout()));await tick();
    reply.resolve(attachmentSchema.parse({...owned,input:null,inputState:'busy'}));await tick(250);state('ready');
    focus(false);fence(true);expect(f.surface.getIdentity()).toBeNull();expect(f.surface.getState()).toBe('blocked');sameInstallation(f);
  });
 });
}
