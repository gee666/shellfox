// Test-build only. Extends the verifier's fake backend without changing its fixtures.
import { FakeNativeBackend } from '../../tests/fixtures/fake-native';
import { success, failure } from '../shared/contracts';
import type { LaunchRequest } from '../shared/native-port';
import type { BoundWindow, MemberRegistration, SessionWindowBackend, SessionWindowEvent } from './platform/session-windows';
import { randomUUID } from 'node:crypto';
export class WindowTestBackend extends FakeNativeBackend implements SessionWindowBackend {
  bindings = new Map<string,BoundWindow>(); members = new Map<string,MemberRegistration>(); states = new Map<string,'alive'|'closed'|'unavailable'>();
  windowListeners = new Set<(event:SessionWindowEvent)=>void>();
  reopens: LaunchRequest[]=[]; preparations: LaunchRequest[]=[];
  constructor() {
    super();
    this.subscribe(event=>{if(event.type==='registered' && event.target){
      const binding={target:event.target,generation:event.registration.operationId};const member={registration:event.registration,wtSession:randomUUID(),binding};
      this.bindings.set(event.registration.sessionId,binding);this.members.set(event.registration.tabId,member);this.states.set(event.registration.sessionId,'alive');
      this.windowEvent({type:'bound',binding});this.windowEvent({type:'adopted',member,previous:null,source:'launch'});
    }});
  }
  override async initialize() { const result=await super.initialize();if(result.ok)result.value.capabilities.addTab=false;return result; }
  windowEvent(event:SessionWindowEvent) { for(const listener of this.windowListeners)listener(event); }
  subscribeSessionWindows(listener:(event:SessionWindowEvent)=>void) {this.windowListeners.add(listener);return()=>{this.windowListeners.delete(listener);};}
  async restoreSessionWindows(input:{bindings:BoundWindow[];members:MemberRegistration[]}) {for(const b of input.bindings){this.bindings.set(b.target.sessionId,b);this.states.set(b.target.sessionId,'alive');}for(const m of input.members)this.members.set(m.registration.tabId,m);return success({restored:true as const});}
  async getWindowMembership(binding:BoundWindow) {return this.bindings.get(binding.target.sessionId)?.generation===binding.generation?success({binding,members:[...this.members.values()].filter(m=>m.registration.sessionId===binding.target.sessionId),windowState:this.states.get(binding.target.sessionId)??'unavailable',discovery:'explicit-registration' as const,reason:'Test-only explicitly registered roots'}):failure('TARGET_LOST','Stale generation');}
  async focusSessionWindow(binding:BoundWindow) {return this.focus(binding.target);}
  async reopenWindow(input:{request:LaunchRequest;previous:BoundWindow}) {
    if(this.states.get(input.request.sessionId)!=='closed' || this.bindings.get(input.request.sessionId)?.generation!==input.previous.generation)return failure('RETRY_CONFIRM_REQUIRED','Closure is not confirmed');
    this.reopens.push(input.request);await new Promise(resolve=>setTimeout(resolve,50));return this.launch(input.request);
  }
  async prepareRegistration(input:{request:LaunchRequest;binding:BoundWindow}) {this.preparations.push(input.request);return success({ticketPath:"C:\\test user's folder\\ticket.json",scriptPath:'C:\\fixture\\register-session.ps1',expiresAt:new Date(Date.now()+120000).toISOString(),titleMarker:input.request.titleMarker});}
  closeWindow(id:string) {
    this.states.set(id,'closed');this.windowEvent({type:'lost',binding:this.bindings.get(id)!,reason:'Synthetic confirmed close'});
    for(const member of [...this.members.values()])if(member.registration.sessionId===id){this.members.delete(member.registration.tabId);this.windowEvent({type:'closed',member});}
  }
  registerPrepared(index:number,existingTabId?:string) {
    const input=this.preparations[index];const previous=existingTabId?this.members.get(existingTabId)??null:null;
    const member:MemberRegistration={binding:this.bindings.get(input.sessionId)!,wtSession:previous?.wtSession??randomUUID(),registration:{sessionId:input.sessionId,tabId:previous?.registration.tabId??input.tabId,operationId:input.operationId,shell:previous?.registration.shell??{pid:9000+index,startTime:'134000000000001000'},cwd:input.cwd,shellExecutable:input.shellExecutable,registeredAt:new Date().toISOString()}};
    this.members.set(member.registration.tabId,member);this.windowEvent({type:'adopted',member,previous,source:'manual'});return member;
  }
}
