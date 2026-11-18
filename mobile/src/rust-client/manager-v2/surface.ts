import type {TerminalConfiguration} from '../configure';
import {encodeTerminalConfiguration} from '../configure';
import {terminalHandleSchema,type TerminalHandle} from '../adapter';
import {encodeTerminalInput,terminalInputSchema,type TerminalInput} from '../input';
import type {NativeTerminalState} from '../native-events';
import type {NativeLayoutPresentation,NativeViewStateListener} from '../native-terminal';
import type {TerminalBridgeState} from '../../terminal/bridge';
import type {UploadIdentity} from '../../terminal/upload';
import type {LayoutComparisonOutcome} from '../../terminal/keyboard-retirement';
import type {ManagerNativeBinding} from './native';
import {attachmentSchema,type ManagerAttachment,type ManagerHostEvent} from './schemas';
import {captureLayoutPresentation,layoutNotice,validateLayoutPresentation,type LayoutPresentation} from './layout-presentation';
interface ViewHost {
  attach():Promise<ManagerAttachment>;
  take(value:ManagerAttachment):Promise<ManagerAttachment>;
  detach(value:ManagerAttachment):Promise<void>;
  current():boolean;
}
export class ManagerSurface {
  private closed=false;
  private closing?:Promise<void>;
  private attaching?:Promise<void>;
  private metadata?:ManagerAttachment;
  // UI continuity context only. It never supplies native readiness or authority.
  private presentationOrigin?:ManagerAttachment;
  private layoutPresentation?:LayoutPresentation;
  private parserReady=false;
  private state:TerminalBridgeState='attaching';
  private readonly listeners=new Set<NativeViewStateListener>();
  private readonly errors=new Set<(error:Error)=>void>();
  private constructor(readonly tabId:string,readonly handle:TerminalHandle,readonly readOnly:boolean,
    private readonly native:ManagerNativeBinding,private readonly client:string,private readonly host:ViewHost,private readonly released:()=>void){}
  static async open(native:ManagerNativeBinding,client:string,tabId:string,readOnly:boolean,configuration:TerminalConfiguration,host:ViewHost,released:()=>void):Promise<ManagerSurface>{
    const handle=terminalHandleSchema.parse(await native.createTerminal(80,24));
    const surface=new ManagerSurface(tabId,handle,readOnly,native,client,host,released);let bound=false;
    try{await surface.configure(configuration);await native.bindTerminal(client,tabId,handle);bound=true;await surface.reattach();return surface;}
    catch(error){
      // A bound failure may own an uncertain host result. Its client retains it.
      if(!bound)await native.closeTerminal(handle);
      throw error;
    }
  }
  get terminalHandle():TerminalHandle{return this.handle;}
  getState():TerminalBridgeState{return this.state;}
  nativeState(value:NativeTerminalState){
    this.parserReady=!this.closed&&value==='ready';
    if(this.parserReady&&this.state==='attached'&&this.host.current()&&this.metadata)
      this.presentationOrigin=attachmentSchema.parse(this.metadata);
    else if(value!=='recovering'&&value!=='ready')this.invalidatePresentation();
  }
  getIdentity():UploadIdentity|null{
    const m=this.metadata;
    return !this.closed&&!this.readOnly&&this.host.current()&&this.parserReady&&this.state==='attached'&&m?.kind==='attached'&&m.inputState==='owned'&&m.input
      ?{tabId:m.input.tabId,generation:m.input.generation,controlEpoch:m.input.controlEpoch}:null;
  }
  attachment():ManagerAttachment|undefined{return this.metadata?attachmentSchema.parse(this.metadata):undefined;}
  subscribe(listener:NativeViewStateListener):()=>void{this.listeners.add(listener);return()=>{this.listeners.delete(listener);};}
  subscribeError(listener:(error:Error)=>void):()=>void{this.errors.add(listener);return()=>{this.errors.delete(listener);};}
  async configure(value:TerminalConfiguration){if(this.closed)throw new Error('Manager surface closed.');await this.native.configureTerminal(this.handle,encodeTerminalConfiguration(value));}
  /** Retires UI continuity only; original network/native work still owns its drain. */
  retirePresentation(outcome?:LayoutComparisonOutcome){
    this.parserReady=false;this.invalidatePresentation();
    if(!this.closed)this.change('reconnecting',outcome?{kind:'layoutRetired',outcome}:undefined);
  }
  reattach():Promise<void>{return this.reattachUnclassified();}
  private reattachUnclassified(outcome?:LayoutComparisonOutcome):Promise<void>{
    if(this.closed)return Promise.reject(new Error('Manager surface closed.'));
    this.retirePresentation(outcome);
    if(this.attaching)return this.attaching;
    if(!this.host.current())return Promise.reject(new Error('Manager surface binding retired.'));
    return this.startAttach();
  }
  private reattachLayout():Promise<void>{
    if(this.attaching)return this.attaching; // Never upgrade unrelated pending work.
    if(!this.host.current()){this.retirePresentation('binding_retired');return Promise.resolve();}
    const presentation=!this.readOnly&&this.host.current()?captureLayoutPresentation(this.metadata,this.presentationOrigin):undefined;
    if(!presentation)return this.reattachUnclassified('before_context_unavailable');
    this.layoutPresentation=presentation;
    return this.startAttach(presentation);
  }
  private startAttach(presentation?:LayoutPresentation):Promise<void>{
    this.parserReady=false;
    const operation=Promise.resolve().then(()=>this.host.attach()).then(value=>{
      const parsed=attachmentSchema.parse(value);
      let outcome:LayoutComparisonOutcome|undefined;
      const observe=(value:LayoutComparisonOutcome)=>{outcome??=value;};
      const comparison=(matches:boolean,value:LayoutComparisonOutcome)=>{if(!matches)observe(value);return matches;};
      const keep=presentation&&comparison(this.layoutPresentation===presentation,'intervening_retirement')
        &&comparison(this.metadata===presentation.source,'source_metadata_replaced')
        &&comparison(!this.closed,'intervening_retirement')&&comparison(this.host.current(),'binding_retired')
        &&validateLayoutPresentation(presentation,parsed,observe);
      this.invalidatePresentation();
      this.accept(parsed,keep?layoutNotice(presentation,'validated'):outcome?{kind:'layoutRetired',outcome}:undefined);
      if(this.attaching===operation)this.attaching=undefined;
    }).catch(error=>{
      this.retirePresentation('request_failed_or_unknown');
      // Retain the original failed/unknown promise. Do not replay its attach.
      throw error;
    });
    this.attaching=operation;
    this.change(this.metadata?'reconnecting':'attaching',presentation?layoutNotice(presentation,'pending'):undefined);
    return operation;
  }
  async takeControl(){
    if(this.closed||this.readOnly)return;
    await this.attaching;if(this.closed)return;
    const current=this.metadata;if(!current)throw new Error('No actual manager delivery.');
    const operation=Promise.resolve().then(()=>this.host.take(current)).then(value=>{
      this.accept(value);if(this.attaching===operation)this.attaching=undefined;
    }).catch(error=>{this.retirePresentation();throw error;});
    this.attaching=operation;this.retirePresentation();await operation;
  }
  async input(raw:TerminalInput){
    if(!this.getIdentity())return;const value=terminalInputSchema.parse(raw);let json:string;
    if(value.type==='key'&&value.key.kind==='character'){
      const text=value.action==='release'?'':value.text??value.key.value;
      json=JSON.stringify({kind:'key',event:{physical:0,action:value.action,modifiers:value.modifiers??0,consumedModifiers:value.consumedModifiers??0,text,unshiftedCodepoint:null,composing:false}});
    }else json=encodeTerminalInput(value);
    await this.native.terminalInput(this.handle,json);
  }
  event(host:ManagerHostEvent){
    if(this.closed||host.type!=='terminal'||this.metadata?.kind!=='attached')return;
    const event=host.event,delivery=this.metadata.delivery;
    if(event.delivery.tabId!==delivery.tabId||event.delivery.generation!==delivery.generation||event.delivery.deliveryId!==delivery.deliveryId||event.delivery.deliveryEpoch!==delivery.deliveryEpoch)return;
    if(event.type==='sourceEnded'||event.type==='processExited'){this.retirePresentation();this.change('closed');}
    else if(event.type==='revoked'){
      this.retirePresentation();this.change(event.reason==='controlLost'?'blocked':'reconnecting');
      if(event.reason==='controlLost')void this.reattach().catch(error=>this.report(error));
    }else if(event.type==='layout'&&event.recoveryRequired)void this.reattachLayout().catch(error=>this.report(error));
    else if(event.type==='error'){this.retirePresentation();this.report(new Error(event.error.message));}
  }
  close():Promise<void>{
    if(this.closing)return this.closing;
    this.closed=true;this.parserReady=false;this.invalidatePresentation();
    this.closing=(async()=>{
      await this.attaching;const value=this.metadata;if(value)await this.host.detach(value);
      await this.native.unbindTerminal(this.client,this.handle);await this.native.closeTerminal(this.handle);this.released();
    })();this.change('closed');return this.closing;
  }
  private invalidatePresentation(){
    if(this.layoutPresentation)this.layoutPresentation.valid=false;
    this.layoutPresentation=undefined;this.presentationOrigin=undefined;
  }
  private accept(value:ManagerAttachment,presentation?:NativeLayoutPresentation):ManagerAttachment{
    const parsed=attachmentSchema.parse(value),tab=parsed.kind==='attached'?parsed.delivery.tabId:parsed.tabId;
    if(tab!==this.tabId)throw new Error('Manager attachment targets another tab.');
    this.metadata=parsed;
    // Only the checked original layout operation may carry UI lineage forward.
    // Input still requires genuine native ready through parserReady/getIdentity.
    if(presentation?.phase==='validated'&&this.host.current())this.presentationOrigin=attachmentSchema.parse(parsed);
    if(!this.closed)this.change(parsed.kind==='unavailable'?'reconnecting':parsed.inputState==='busy'?'blocked':'attached',presentation);
    return parsed;
  }
  private report(value:unknown){
    this.retirePresentation();const error=value instanceof Error?value:new Error('Manager surface failed.');
    for(const listener of this.errors){try{listener(error);}catch{}}
  }
  private change(state:TerminalBridgeState,presentation?:NativeLayoutPresentation){
    this.state=state;for(const listener of this.listeners){try{listener(state,presentation);}catch{}}
  }
}
