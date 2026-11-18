import { z } from 'zod';
import { remotePeerIdSchema } from '../../../../src/shared/remote-schemas';
import { observerId as id, disconnectedSchema } from '../observer-v2/primitives';
import { positiveDecimalSchema } from '../identifiers';
import { CoarseOperations } from '../coarse-operations';
import { nativeManagerEventSchema, type ManagerNativeBinding, type NativeManagerEvent } from './native';
import { bindingSchema, discoveryOutcomeSchema, pairSelectionSchema, reply, sameBinding,
  attachmentSchema, attachSchema, takeSchema, detachSchema, pollSchema,
  type ManagerBinding, type ManagerAttachment } from './schemas';
import { command, parseResponse, type MetadataMethod, type ManagerPayload, type ManagerResponse } from './methods';
import { ManagerSurface } from './surface';
import type { TerminalConfiguration } from '../configure';
import type { NativeViewDriver } from '../native-terminal';
const pairedReply=reply(pairSelectionSchema);
type PairReply=z.infer<typeof pairedReply>;
const disconnectedReply=reply(disconnectedSchema);
type DisconnectReply=z.infer<typeof disconnectedReply>;
type CoarseEvents=Extract<z.infer<typeof pollSchema>,{ok:true}>['value']['events'];
export type ManagerState=Readonly<{status:NativeManagerEvent['connection']['status'];binding:ManagerBinding|null}>;
export class ManagerV2Client{
  readonly profile='manager-v2';readonly apiCapabilities=7;
  readonly terminals:NativeViewDriver={open:(tab,readOnly,configuration)=>this.openTerminal(tab,readOnly,configuration)};
  private lifecycle=0;private identity:string;private closed=false;private closing?:Promise<void>;private polling=false;private pairing?:Promise<PairReply>;
  private admissionClosed=false;private disconnecting?:Promise<DisconnectReply>;
  private state:ManagerState={status:'disconnected',binding:null};private discovery?:z.infer<typeof discoveryOutcomeSchema>;
  private selection?:z.infer<typeof pairSelectionSchema>;
  private readonly listeners=new Set<(state:ManagerState)=>void>();
  private readonly changes=new Set<(event:CoarseEvents[number])=>void>();
  private readonly surfaces=new Map<string,ManagerSurface>();private readonly opening=new Map<string,Promise<ManagerSurface>>();
  private readonly pump:CoarseOperations<NativeManagerEvent>;
  private constructor(private readonly native:ManagerNativeBinding,readonly handle:string,identity:string){
    this.identity=identity;this.pump=new CoarseOperations({read:()=>this.read(),cancel:operation=>native.cancel(handle,operation),close:()=>native.closeClient(handle)},event=>this.observe(event),()=>this.setState({status:'reconnecting',binding:null}),9);this.pump.start();
  }
  static async create(native:ManagerNativeBinding,peer:string,identity:string):Promise<ManagerV2Client>{
    if(native.abiVersion()!==1||native.managerV2AbiVersion()!==1)throw new Error('This binary has no compatible Native0 Manager client.');
    const actual=id.parse(identity).toLowerCase();const handle=positiveDecimalSchema.parse(await native.createManagerV2Client(remotePeerIdSchema.parse(peer),actual));
    try{const encoded=await native.clientProfile(handle);if(encoded.length>1024)throw new Error('Invalid native profile.');z.object({profile:z.literal(3),apiCapabilities:z.literal(7)}).strict().parse(JSON.parse(encoded));return new ManagerV2Client(native,handle,actual);}
    catch(error){await native.closeClient(handle).catch(()=>undefined);throw error;}
  }
  getState():ManagerState{return{status:this.state.status,binding:this.state.binding?{...this.state.binding}:null};}
  subscribeState(listener:(state:ManagerState)=>void):()=>void{this.listeners.add(listener);return()=>{this.listeners.delete(listener);};}
  subscribe(listener:(event:CoarseEvents[number])=>void):()=>void{this.changes.add(listener);return()=>{this.changes.delete(listener);};}
  async discover(){this.live();const event=await this.pump.invoke(()=>this.native.discoverManagerV2(this.handle));this.discovery=discoveryOutcomeSchema.parse(event.result);return this.discovery;}
  async pair(attemptId:string):Promise<PairReply>{
    this.live();if(this.pairing)throw new Error('Manager pair is already pending.');const discovery=this.discovery;if(!discovery||!('kind'in discovery)||discovery.kind!=='available')throw new Error('Discover the available Manager profile before pairing.');const attempt=id.parse(attemptId).toLowerCase();
    const operation=(async()=>{const event=await this.pump.invoke(()=>this.native.pairManagerV2(this.handle,attempt));const result=pairedReply.parse(event.result);if(result.ok){const s=result.value;if(s.clientId!==this.identity||s.ownerId!==discovery.discovery.ownerId||s.attemptId!==attempt)throw new Error('Manager pair echo mismatch.');const binding=bindingSchema.parse({clientId:s.clientId,ownerId:s.ownerId,connectionId:s.connectionId,profile:s.profile});if(!event.connection.binding||!sameBinding(binding,event.connection.binding))throw new Error('Manager pair binding mismatch.');this.selection=s;this.setState({status:'connected',binding});}return result;
    })().finally(()=>{if(this.pairing===operation)this.pairing=undefined;});
    this.pairing=operation;return operation;
  }
  async request<M extends MetadataMethod>(method:M,payload:ManagerPayload<M>):Promise<ManagerResponse<M>>{
    const binding=this.connected(),json=command(method,payload);const event=await this.pump.invoke(()=>this.native.managerV2Operation(this.handle,json));const result=parseResponse(method,event.result);if(result.ok)this.check(event,binding);return result;
  }
  async attach(tabId:string,control:'none'|'ifFree'='ifFree'):Promise<ManagerAttachment>{return this.viewRequest('attachTerminal',attachSchema.parse({tabId,resume:null,control}));}
  async take(attachment:ManagerAttachment):Promise<ManagerAttachment>{if(attachment.kind!=='attached')throw new Error('No actual delivery to take.');return this.viewRequest('takeTerminalControl',takeSchema.parse({delivery:attachment.delivery,mode:'takeover'}));}
  private async viewRequest(method:'attachTerminal'|'takeTerminalControl',payload:z.infer<typeof attachSchema>|z.infer<typeof takeSchema>,original?:ManagerBinding):Promise<ManagerAttachment>{
    const binding=original??this.connected(),selection=this.selection;
    if(original&&!sameBinding(original,this.connected(true)))throw new Error('Manager opening belongs to a retired binding.');if(!selection)throw new Error('Manager selection unavailable.');const event=await this.pump.invoke(()=>this.native.managerV2Operation(this.handle,JSON.stringify({method,payload})));const result=reply(attachmentSchema).parse(event.result);if(!result.ok)throw new Error(result.error.message);this.check(event,binding);if(result.value.kind==='attached'&&Object.entries(selection.native0Sender).some(([key,value])=>Reflect.get(result.value.kind==='attached'?result.value.recovery.metadata.codec:{},key)!==value))throw new Error('Native recovery differs from negotiated sender.');return result.value;
  }
  async detach(attachment:ManagerAttachment):Promise<void>{if(attachment.kind!=='attached')return;const binding=this.connected(true);const payload=detachSchema.parse({delivery:attachment.delivery,releaseControl:attachment.input});const event=await this.pump.invoke(()=>this.native.managerV2Operation(this.handle,JSON.stringify({method:'detachTerminal',payload})));const result=reply(z.object({detached:z.literal(true)}).strict()).parse(event.result);if(!result.ok)throw new Error(result.error.message);this.check(event,binding);}
  async poll(){this.connected();if(this.polling)throw new Error('Manager poll is already pending.');this.polling=true;const binding=this.connected();try{const event=await this.pump.invoke(()=>this.native.pollManagerV2(this.handle));const result=pollSchema.parse(event.result);if(result.ok){this.check(event,binding);if(!event.internal)this.deliver(result.value.events);}return result;}finally{this.polling=false;}}
  async openTerminal(tabId:string,readOnly:boolean,configuration:TerminalConfiguration):Promise<ManagerSurface>{
    const tab=id.parse(tabId),lifecycle=this.lifecycle,binding=this.connected();if(this.opening.has(tab)||this.surfaces.has(tab))throw new Error('Manager terminal already open.');
    const operation=ManagerSurface.open(this.native,this.handle,tab,readOnly,configuration,{attach:()=>this.viewRequest('attachTerminal',attachSchema.parse({tabId:tab,resume:null,control:readOnly?'none':'ifFree'}),binding),take:value=>this.take(value),detach:value=>this.detach(value),current:()=>this.presentationCurrent(binding)},()=>this.surfaces.delete(tab)).then(async surface=>{
      if(lifecycle!==this.lifecycle||this.closed){await surface.close();throw new Error('Manager client changed while opening terminal.');}
      this.surfaces.set(tab,surface);return surface;
    }).then(surface=>{if(this.opening.get(tab)===operation)this.opening.delete(tab);return surface;});
    this.opening.set(tab,operation);return operation;
  }
  private retirePresentations(){for(const view of this.surfaces.values())view.retirePresentation();}
  private presentationCurrent(binding:ManagerBinding):boolean{return !this.closed&&!this.closing&&!this.admissionClosed&&this.state.status==='connected'&&this.state.binding!==null&&sameBinding(binding,this.state.binding);}
  private async retireViews(){const pairing=this.pairing;if(pairing){const reply=await pairing;if(!reply.ok)throw new Error(reply.error.message);}await Promise.all([...this.opening.values()]);await Promise.all([...this.surfaces.values()].map(view=>view.close()));}
  async reconnect(identity:string):Promise<void>{
    const retirement=this.disconnecting;if(retirement){const reply=await retirement;if(!reply.ok)throw new Error(reply.error.message);}else this.live();
    if(this.closed||this.closing)throw new Error('Manager client is closed.');
    this.admissionClosed=true;this.retirePresentations();await this.retireViews();this.lifecycle++;
    await this.native.reconnectManagerV2(this.handle,id.parse(identity));
    if(this.disconnecting===retirement)this.disconnecting=undefined;
    this.identity=identity.toLowerCase();this.discovery=undefined;this.selection=undefined;this.admissionClosed=false;this.setState({status:'reconnecting',binding:null});
  }
  disconnect():Promise<DisconnectReply>{
    if(this.disconnecting)return this.disconnecting;
    try{this.live();}catch(error){return Promise.reject(error);}this.admissionClosed=true;
    const operation=Promise.resolve().then(async()=>{await this.retireViews();this.lifecycle++;
      const event=await this.pump.invoke(()=>this.native.disconnectManagerV2(this.handle));const result=disconnectedReply.parse(event.result);
      if(result.ok)this.setState({status:'disconnected',binding:null});return result;
    });this.disconnecting=operation;this.retirePresentations();return operation;
  }
  close():Promise<void>{
    if(this.closing)return this.closing;if(this.closed)return Promise.resolve();this.admissionClosed=true;
    const operation=Promise.resolve().then(async()=>{await this.retireViews();const retirement=await this.disconnecting;if(retirement&&!retirement.ok)throw new Error(retirement.error.message);this.lifecycle++;
      await this.pump.close();this.closed=true;this.setState({status:'disconnected',binding:null});
    });this.closing=operation;this.retirePresentations();return operation;
  }
  private connected(cleanup=false):ManagerBinding{if(cleanup){if(this.closed)throw new Error('Manager client is closed.');}else this.live();if(this.state.status!=='connected'||!this.state.binding)throw new Error('Manager client is not connected.');return{...this.state.binding};}
  private live(){if(this.closed||this.closing||this.admissionClosed)throw new Error('Manager client is retiring or closed.');}
  private check(event:NativeManagerEvent,binding:ManagerBinding){if(!event.connection.binding||!sameBinding(binding,event.connection.binding)||!this.state.binding||!sameBinding(binding,this.state.binding))throw new Error('Manager response belongs to a stale connection.');}
  private setState(state:ManagerState){if(state.status!=='connected'||!state.binding||!this.state.binding||!sameBinding(state.binding,this.state.binding))this.retirePresentations();this.state=state;for(const listener of this.listeners){try{listener(this.getState());}catch{}}}
  private async read(){const encoded=await this.native.readEvent(this.handle);if(encoded===null)return null;if(new TextEncoder().encode(encoded).length>8388608+65536)throw new Error('Manager event exceeds limit.');return nativeManagerEventSchema.parse(JSON.parse(encoded));}
  private observe(event:NativeManagerEvent){this.setState(event.connection);if(event.internal&&event.status===0&&event.result){const result=pollSchema.safeParse(event.result);if(result.success&&result.data.ok)this.deliver(result.data.value.events);}}
  private deliver(events:CoarseEvents){for(const event of events){if(event.type==='terminal')this.surfaces.get(event.event.delivery.tabId)?.event(event);for(const listener of this.changes){try{listener(event);}catch{}}}}
}
