import React, {type ReactNode} from 'react';
import {act, cleanup, fireEvent, render, screen} from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import {afterEach, beforeEach, describe, expect, it, vi} from 'vitest';
import {RustTerminalView} from '../../terminal/RustTerminalView';

interface UiProps {children?: ReactNode; accessibilityLabel?: string; onPress?: () => void; disabled?: boolean; accessibilityState?: {selected?: boolean}}
interface NativeProbe {
  terminalHandle: string; readOnly?: boolean; focused?: boolean; modifiers?: number;
  onState?: (event: {nativeEvent: {terminalHandle: string; state: string}}) => void;
  onError?: (event: {nativeEvent: {terminalHandle: string; code: number}}) => void;
}
const probe = vi.hoisted(() => ({platform:'android' as 'android'|'ios', visible:false,
  native:undefined as NativeProbe|undefined, keyboardPress:undefined as (()=>void)|undefined,
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
vi.mock('../../terminal/useKeyboardOverlap',()=>({useKeyboardOverlap:()=>({keyboardInset:0,keyboardOffset:0,
  keyboardVisible:probe.visible,measureKeyboardInset:()=>undefined})}));
vi.mock('../../terminal/useTerminalAttachment',()=>({useTerminalAttachment:()=>({invalidate:probe.invalidate,panel:null,button:undefined})}));
vi.mock('@react-native-clipboard/clipboard',()=>({default:{setString:vi.fn(),getString:async()=>''}}));
vi.mock('../specs/ShellfoxRustTerminalNativeComponent',()=>({default:(props:NativeProbe)=>{
  probe.native=props;
  return <div aria-label="Native terminal" data-readonly={String(props.readOnly)} data-focused={String(props.focused)} data-modifiers={props.modifiers}>
    {['bound','ready','recovering','closed','invalid'].map(state=><button key={state} aria-label={`Native state ${state}`}
      onClick={()=>props.onState?.({nativeEvent:{terminalHandle:props.terminalHandle,state}})}/>)}
  </div>;
}}));

import fs from 'node:fs';
import {z} from 'zod';
import {ManagerV2Client} from './client';
import {ManagerLifecycleNative,CLIENT,ATTEMPT,TAB} from './test-native';
import {nativeManagerEventSchema,type NativeManagerEvent} from './native';
import {attachmentSchema,controlSchema,deliverySchema,layoutSchema,nativeRecoverySchema,terminalEventSchema,
  type ManagerAttachment,type ManagerHostEvent} from './schemas';
import type {NativeViewDriver} from '../native-terminal';
const readFixture=(name:string):unknown=>JSON.parse(fs.readFileSync(`crates/shellfox-protocol/tests/fixtures/manager_v2/${name}.json`,'utf8'));
// Inert canonical metadata only. No decoder, SDK authority, wire grant or ACK is exercised.
const original=z.object({value:z.object({delivery:deliverySchema,input:controlSchema,layout:layoutSchema}).passthrough()}).passthrough().parse(readFixture('attachment')).value;
const recoveryFile=z.object({data:z.string()}).passthrough().parse(readFixture('recovery-native0'));
const {data:_bytes,...metadata}=recoveryFile;
const owned=attachmentSchema.parse({kind:'attached',delivery:original.delivery,input:original.input,layout:original.layout,
  inputState:'owned',recovery:nativeRecoverySchema.parse(metadata)});
function deferred<T>(){let resolve!:(value:T)=>void,reject!:(cause:Error)=>void;const promise=new Promise<T>((yes,no)=>{resolve=yes;reject=no;});return{promise,resolve,reject};}
const clients:ManagerV2Client[]=[];
async function tick(ms=150){await act(async()=>{await vi.advanceTimersByTimeAsync(ms);});}
function nativeFixture(){
  const native=new ManagerLifecycleNative(),queued:NativeManagerEvent[]=[];
  let connection:NativeManagerEvent['connection']={status:'disconnected',binding:null},sequence=20000;
  let next:Promise<ManagerAttachment>|undefined,replyConnection:NativeManagerEvent['connection']|undefined;
  const inheritedRead=native.readEvent.getMockImplementation();if(!inheritedRead)throw Error('Original inert reader absent');
  native.readEvent.mockImplementation(async()=>{
    const event=queued.shift();const encoded=event?JSON.stringify(event):await inheritedRead();
    if(encoded!==null)connection=nativeManagerEventSchema.parse(JSON.parse(encoded)).connection;
    return encoded;
  });
  native.managerV2Operation.mockImplementation(async(_client,json)=>{
    const command=z.object({method:z.string(),payload:z.unknown()}).strict().parse(JSON.parse(json));native.calls.push(command.method);
    let value:unknown;
    if(command.method==='attachTerminal'){const pending=next;next=undefined;value=pending?await pending:owned;}
    else if(command.method==='detachTerminal')value={detached:true};
    else throw Error('Unexpected inert operation');
    const operationId=String(sequence++);queued.push({profile:'manager-v2',operationId,internal:false,status:0,result:{version:2,ok:true,value},connection:replyConnection??connection});return operationId;
  });
  return{native,pending:(value:Promise<ManagerAttachment>)=>{next=value;},differentReplyConnection:(value:NativeManagerEvent['connection'])=>{replyConnection=value;}};
}
async function setup(){
  const fixture=nativeFixture(),client=await ManagerV2Client.create(fixture.native,'inert-offline-layout-fixture',CLIENT);clients.push(client);
  const discovery=client.discover();await tick();await discovery;const pairing=client.pair(ATTEMPT);await tick();await pairing;
  const opening=client.openTerminal(TAB,false,{foreground:'#ffffff',background:'#000000'});await tick(250);const surface=await opening;
  const driver:NativeViewDriver={open:vi.fn(async()=>surface)};
  const props={driver,tabId:TAB,fontSize:14,showKeyboardDiagnostics:true,connectionEpoch:client.getState().binding?.connectionId??'missing'};
  const rendered=render(<RustTerminalView {...props}/>);await tick();state('ready');
  const binding=client.getState().binding,identity=surface.getIdentity(),terminal=screen.getByLabelText('Native terminal');
  expect(binding).not.toBeNull();expect(identity).not.toBeNull();
  return{...fixture,client,surface,driver,binding,identity,terminal,refresh:()=>rendered.rerender(<RustTerminalView {...props}/>)};
}
function state(value:string){fireEvent.click(screen.getByRole('button',{name:`Native state ${value}`}));}
function keyboard(){fireEvent.click(screen.getByRole('button',{name:'Keyboard'}));}
function layout(delivery=original.delivery,recoveryRequired=true,layoutRevision=2):ManagerHostEvent{return{type:'terminal',event:terminalEventSchema.parse({type:'layout',delivery,layoutRevision,cols:80,rows:20,recoveryRequired,lastSequence:0})};}
function focus(value:boolean){expect(screen.getByLabelText('Native terminal')).toHaveAttribute('data-focused',String(value));}
function fence(value:boolean){expect(screen.getByLabelText('Native terminal')).toHaveAttribute('data-readonly',String(value));}
function sameInstallation(f:Awaited<ReturnType<typeof setup>>){
  expect(f.client.getState()).toEqual({status:'connected',binding:f.binding});expect(f.driver.open).toHaveBeenCalledTimes(1);
  expect(f.native.createManagerV2Client).toHaveBeenCalledTimes(1);expect(f.native.pairManagerV2).toHaveBeenCalledTimes(1);
  expect(f.native.createTerminal).toHaveBeenCalledTimes(1);expect(f.native.bindTerminal).toHaveBeenCalledTimes(1);
  expect(f.native.closeTerminal).not.toHaveBeenCalled();expect(f.native.unbindTerminal).not.toHaveBeenCalled();expect(f.native.terminalInput).not.toHaveBeenCalled();
  expect(f.native.reconnectManagerV2).not.toHaveBeenCalled();expect(f.native.disconnectManagerV2).not.toHaveBeenCalled();
  expect(f.native.pollManagerV2).not.toHaveBeenCalled();expect(f.native.progress).not.toHaveBeenCalled();
}
beforeEach(()=>{vi.useFakeTimers();vi.clearAllMocks();probe.visible=false;probe.native=undefined;probe.keyboardPress=undefined;});
afterEach(async()=>{cleanup();for(const client of clients.splice(0)){const closing=client.close().catch(()=>undefined);await vi.advanceTimersByTimeAsync(500);await closing;}vi.useRealTimers();});

export {probe,original,owned,deferred,tick,setup,state,keyboard,layout,focus,fence,sameInstallation};
