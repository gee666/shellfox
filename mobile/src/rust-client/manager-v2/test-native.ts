import fs from 'node:fs';
import {vi} from 'vitest';
import {z} from 'zod';
import {TestNative} from '../test-native';
import {discoverySchema} from '../observer-v2/primitives';
import {bindingSchema,codecSchema,nativeRecoverySchema,pairSelectionSchema} from './schemas';
import type {ManagerNativeBinding,NativeManagerEvent} from './native';
export const CLIENT='20000000-0000-4000-8000-000000000001';
export const ATTEMPT='30000000-0000-4000-8000-000000000001';
export const TAB='50000000-0000-4000-8000-000000000001';
const fixture=(name:string):unknown=>JSON.parse(fs.readFileSync(`crates/shellfox-protocol/tests/fixtures/manager_v2/${name}.json`,'utf8'));
const recoveryInput=z.object({data:z.string()}).passthrough().parse(fixture('recovery-native0'));
const {data:_data,...recoveryMetadata}=recoveryInput;
const recovery=nativeRecoverySchema.parse(recoveryMetadata),codec=codecSchema.parse(recovery.metadata.codec);
const discovery=z.object({value:discoverySchema}).passthrough().parse(fixture('discovery')).value;
const basePair=z.object({value:z.object({ownerId:z.string(),connectionId:z.string()}).passthrough()}).passthrough().parse(fixture('pair-selection')).value;
/** Inert lifecycle fixture only. No socket, decoder, real receipt or authority. */
export class ManagerLifecycleNative extends TestNative implements ManagerNativeBinding {
  readonly calls:string[]=[];
  pairBarrier?:Promise<void>;
  attachBarrier?:Promise<void>;
  detachBarrier?:Promise<void>;
  attachStatus=0;
  pollStatus=0;
  private identity=CLIENT;
  private sequence=1000;
  private readonly completed:NativeManagerEvent[]=[];
  private current:NativeManagerEvent['connection']={status:'disconnected',binding:null};
  readonly managerV2AbiVersion=vi.fn(()=>1);
  readonly clientProfile=vi.fn(async()=>JSON.stringify({profile:3,apiCapabilities:7}));
  readonly createManagerV2Client=vi.fn(async(_peer:string,identity:string)=>{this.identity=identity;return '2';});
  readonly discoverManagerV2=vi.fn(async()=>this.finish({kind:'available',discovery:{...discovery,recoveryFormats:['native0-raw-v1']}}));
  readonly pairManagerV2=vi.fn(async(_client:string,attempt:string)=>{
    await this.pairBarrier;
    const selection=pairSelectionSchema.parse({...basePair,clientId:this.identity,attemptId:attempt,profile:'manager-v2',catalogMode:'complete',deliveryFormat:'vt-bytes-base64-v1',recoveryFormat:'native0-raw-v1',inputFormat:'manager-input-v1',devicePolicy:'control-full-account-v1',devicePermission:'write',native0Sender:codec,native0Receiver:codec});
    this.current={status:'connected',binding:bindingSchema.parse({clientId:selection.clientId,ownerId:selection.ownerId,connectionId:selection.connectionId,profile:selection.profile})};
    return this.finish({version:2,ok:true,value:selection});
  });
  readonly managerV2Operation=vi.fn(async(_client:string,json:string)=>{
    const command=z.object({method:z.string(),payload:z.unknown()}).strict().parse(JSON.parse(json));this.calls.push(command.method);
    if(command.method==='attachTerminal'){
      await this.attachBarrier;
      return this.finish({version:2,ok:true,value:{kind:'attached',delivery:{tabId:TAB,generation:'60000000-0000-4000-8000-000000000001',deliveryId:'70000000-0000-4000-8000-000000000001',deliveryEpoch:7},input:null,inputState:'intrinsicReadOnly',layout:null,recovery}},this.attachStatus);
    }
    if(command.method==='detachTerminal'){await this.detachBarrier;return this.finish({version:2,ok:true,value:{detached:true}});}
    throw new Error('Unexpected lifecycle fixture operation');
  });
  readonly pollManagerV2=vi.fn(async()=>this.finish({version:2,ok:true,value:{events:[]}},this.pollStatus));
  readonly reconnectManagerV2=vi.fn(async(_client:string,identity:string)=>{this.calls.push('reconnect');this.identity=identity;this.current={status:'reconnecting',binding:null};});
  readonly disconnectManagerV2=vi.fn(async()=>{this.calls.push('disconnect');this.current={status:'disconnected',binding:null};return this.finish({version:2,ok:true,value:{disconnected:true}});});
  override readonly readEvent=vi.fn(async()=>{const event=this.completed.shift();return event?JSON.stringify(event):null;});
  private finish(result:unknown,status=0):string {
    const operationId=String(this.sequence++);this.completed.push({profile:'manager-v2',operationId,internal:false,status,result,connection:this.current});return operationId;
  }
}
