import { describe, it, expect } from 'vitest';
import { randomUUID } from 'node:crypto';
import { SessionService } from './service';
import { defaultSettings, unavailableProbe } from './defaults';
import type { RepositoryPort, SessionRecord, TabRecord, OperationRecord } from './models';
import type { BoundWindow, MemberRegistration, SessionWindowBackend, SessionWindowEvent } from './platform/session-windows';
import type { NativeBackend, NativeEvent, LaunchRequest, WindowTarget } from '../shared/native-port';
import type { NativeProbe, Result, SettingsDto, SessionDto } from '../shared/contracts';
import { success, failure } from '../shared/contracts';

class MemoryRepository implements RepositoryPort {
  sessionMap = new Map<string, SessionRecord>(); tabMap = new Map<string, TabRecord>(); operationMap = new Map<string, OperationRecord>();
  config = structuredClone(defaultSettings); preference = false;
  sessions() { return [...this.sessionMap.values()].map(s => structuredClone(s)); }
  session(id: string) { const s = this.sessionMap.get(id); return s && structuredClone(s); }
  tabs(id?: string) { return [...this.tabMap.values()].filter(t => !id || t.sessionId===id).sort((a,b) => a.ordinal-b.ordinal).map(t => structuredClone(t)); }
  tab(id: string) { const t = this.tabMap.get(id); return t && structuredClone(t); }
  saveSession(s: SessionRecord) { this.sessionMap.set(s.id,structuredClone(s)); }
  saveTab(t: TabRecord) { this.tabMap.set(t.id,structuredClone(t)); }
  saveOperation(o: OperationRecord) { this.operationMap.set(o.id,structuredClone(o)); }
  operation(id: string) { const o = this.operationMap.get(id); return o && structuredClone(o); }
  operationsForTab(id: string) { return [...this.operationMap.values()].filter(o => o.tabId === id).map(o => structuredClone(o)); }
  sessionByRequest(id: string) { const o = [...this.operationMap.values()].find(o => o.requestId===id); return o && this.session(o.sessionId); }
  transaction<T>(fn: () => T) { return fn(); }
  settings() { return structuredClone(this.config); }
  saveSettings(s: SettingsDto) { this.config=structuredClone(s); }
  settled(search: string) { return this.sessions().filter(s => s.settledAt && (s.title.toLowerCase().includes(search.toLowerCase()) || s.cwd.toLowerCase().includes(search.toLowerCase()))).sort((a,b) => b.settledAt!.localeCompare(a.settledAt!) || a.id.localeCompare(b.id)); }
  explorerPreference() { return this.preference; }
  saveExplorerPreference(v: boolean) { this.preference=v; }
  close() {}
}
function fixture(directoryValidator: (cwd: string) => Promise<string> = async cwd => cwd, extension = false) {
  const repository = new MemoryRepository();
  const listeners = new Set<(event: NativeEvent) => void>();
  const launches: LaunchRequest[] = [];
  const windowListeners = new Set<(event: SessionWindowEvent)=>void>();
  const bindings = new Map<string,BoundWindow>();
  const members = new Map<string,MemberRegistration>();
  const windowStates = new Map<string,'alive'|'closed'|'unavailable'>();
  const reopens: LaunchRequest[]=[]; const preparations: LaunchRequest[]=[];
  const windowEvent=(event:SessionWindowEvent)=>{for(const listener of windowListeners)listener(event);};
  let launchError: Result<never> | undefined;
  let autoRegister = true;
  let receiptNull = false;
  let denial = false;
  let watchCount = 0;
  const probe: NativeProbe = { ...unavailableProbe(''), available:true, reasons:[], shells:[{id:'pwsh',executable:'C:\\PowerShell\\pwsh.exe',available:true,reason:null}], capabilities:{...unavailableProbe('').capabilities,createWindow:true,addTab:true,focusWindow:true,processTracking:true} };
  const target = (input: LaunchRequest): WindowTarget => ({kind:'windows-terminal',sessionId:input.sessionId,windowName:input.windowName,markerPrefix:`SHELLFOX:${input.sessionId}:`,hwnd:extension?String(12345+launches.indexOf(input)):'12345',owner:{pid:extension?100+launches.indexOf(input):100,startTime:'123456'},verification:'native-title'});
  const emit = (event: NativeEvent) => { for(const listener of listeners) listener(event); };
  const register = (input: LaunchRequest) => {
    const registration={sessionId:input.sessionId,tabId:input.tabId,operationId:input.operationId,shell:{pid:200+launches.indexOf(input),startTime:'123456789'},shellExecutable:input.shellExecutable,cwd:input.cwd,registeredAt:new Date().toISOString()};
    if(extension){const binding={target:target(input),generation:input.operationId};const member={registration,wtSession:randomUUID(),binding};bindings.set(input.sessionId,binding);members.set(input.tabId,member);windowStates.set(input.sessionId,'alive');windowEvent({type:'bound',binding});windowEvent({type:'adopted',member,previous:null,source:'launch'});}
    emit({type:'registered',registration,target:target(input)});
  };
  const backend: NativeBackend = {
    initialize: async () => success(probe),
    launch: async input => { launches.push(input); if(launchError)return launchError; if(autoRegister)register(input); return success({operationId:input.operationId,dispatch:'started',target:receiptNull?null:target(input)}); },
    focus: async () => denial ? failure('FOCUS_DENIED','Switch manually',true) : success({focused:true}),
    verifyTarget: async input => success(input),
    setWatch: async () => {watchCount++; return success({configured:true});},
    getExplorerIntegration: async () => success({supported:false,installed:false,folderItemInstalled:false,backgroundInstalled:false,reason:null}),
    setExplorerIntegration: async () => failure('UNSUPPORTED','Unavailable'),
    subscribe: fn => {listeners.add(fn); return () => {listeners.delete(fn);};}, dispose:async () => {},
  };
  if(extension)Object.assign(backend,{
    reopenWindow:async({request,previous}:Parameters<SessionWindowBackend['reopenWindow']>[0])=>{
      reopens.push(request);
      if(bindings.get(request.sessionId)?.generation!==previous.generation || windowStates.get(request.sessionId)!=='closed')return failure('RETRY_CONFIRM_REQUIRED','Not confirmed closed');
      return backend.launch(request);
    },
    focusSessionWindow:async(binding:BoundWindow)=>backend.focus(binding.target),
    prepareRegistration:async({request}:Parameters<SessionWindowBackend['prepareRegistration']>[0])=>{preparations.push(request);return success({ticketPath:"C:\\tmp\\a'b\\ticket.json",scriptPath:'C:\\app\\register-session.ps1',expiresAt:new Date(Date.now()+120000).toISOString(),titleMarker:request.titleMarker});},
    getWindowMembership:async(binding:BoundWindow)=>bindings.get(binding.target.sessionId)?.generation===binding.generation?success({binding,members:[...members.values()].filter(m=>m.registration.sessionId===binding.target.sessionId),windowState:windowStates.get(binding.target.sessionId)??'unavailable',discovery:'explicit-registration' as const,reason:'Explicit registrations only'}):failure('TARGET_LOST','Stale generation'),
    restoreSessionWindows:async(input:{bindings:BoundWindow[];members:MemberRegistration[]})=>{for(const b of input.bindings)bindings.set(b.target.sessionId,b);for(const m of input.members)members.set(m.registration.tabId,m);return success({restored:true as const});},
    subscribeSessionWindows:(listener:(event:SessionWindowEvent)=>void)=>{windowListeners.add(listener);return()=>{windowListeners.delete(listener);};},
  } satisfies Partial<SessionWindowBackend>);
  const closeWindow=(id:string)=>{
    windowStates.set(id,'closed');windowEvent({type:'lost',binding:bindings.get(id)!,reason:'Confirmed closed'});
    for(const member of [...members.values()])if(member.registration.sessionId===id){members.delete(member.registration.tabId);windowEvent({type:'closed',member});}
  };
  const adoptManual=(index:number,previous:MemberRegistration|null=null,emitEvent=true)=>{
    const input=preparations[index];const member:MemberRegistration={binding:bindings.get(input.sessionId)!,wtSession:previous?.wtSession??randomUUID(),registration:{sessionId:input.sessionId,tabId:previous?.registration.tabId??input.tabId,operationId:input.operationId,cwd:input.cwd,shellExecutable:input.shellExecutable,shell:previous?.registration.shell??{pid:900+index,startTime:'900000'},registeredAt:new Date().toISOString()}};
    members.set(member.registration.tabId,member);if(emitEvent)windowEvent({type:'adopted',member,previous,source:'manual'});return member;
  };
  const service = new SessionService(repository,backend,directoryValidator);
  const start = () => service.initialize({userDataDir:'C:\\tmp',helperDir:'C:\\helper',shellScriptDir:'C:\\shell',packagedExecutable:null});
  const create = async (requestId=randomUUID()) => {
    const session = value(await service.createSession({cwd:'C:\\same',requestId}));
    await new Promise(resolve => setImmediate(resolve));
    return session;
  };
  const observe = (s: SessionDto, agents: number, root: 'alive'|'exited'|'unavailable'='alive', health: 'healthy'|'unknown'='healthy') => emit({type:'observations',items:s.tabs.map(t => ({sessionId:s.id,tabId:t.id,observedAt:new Date().toISOString(),root,health,agents,reason:null}))});
  return {repository,service,backend,launches,bindings,members,windowStates,reopens,preparations,windowEvent,closeWindow,adoptManual,start,create,observe,emit,register,target,probe,setError:(e:Result<never>|undefined)=>{launchError=e;},setRegister:(v:boolean)=>{autoRegister=v;},setReceiptNull:()=>{receiptNull=true;},setDenial:(v=true)=>{denial=v;},watchCount:()=>watchCount};
}
function value<T>(result: Result<T>): T { if(!result.ok)throw new Error(result.error.code); return result.value; }
describe('session service with mock native transport and in-memory repository', () => {
  it('clears only recovered focus denial on successful extended focus so live agent activity is visible', async () => {
    const f=fixture(undefined,true);await f.start();const s=await f.create();f.observe(s,1);f.setDenial();
    expect(await f.service.activateSession({sessionId:s.id})).toMatchObject({ok:false,error:{code:'FOCUS_DENIED'}});
    expect(value(f.service.getSnapshot()).sessions[0]!.status).toBe('error');
    f.setDenial(false);const restored=value(await f.service.activateSession({sessionId:s.id}));
    expect(restored.error).toBeNull();expect(restored.status).toBe('running');expect(restored.counts.agents).toBe(1);
  });
  it('deduplicates request identity, not CWD, and persists exactly one initial tab', async () => {
    const f=fixture(); await f.start(); const id=randomUUID();
    const [a,b]=await Promise.all([f.create(id),f.create(id)]); const c=await f.create();
    expect(a.id).toBe(b.id); expect(c.id).not.toBe(a.id); expect(f.launches).toHaveLength(2);
    expect(f.repository.sessions()).toHaveLength(2); expect(a.tabs).toHaveLength(1);
    expect(f.repository.operation(a.tabs[0].id)).toBeUndefined();
    expect([...f.repository.operationMap.values()].filter(o=>o.requestId===id)).toHaveLength(1);
  });
  it('does not create unsupported or invalid folder intents', async () => {
    const f=fixture(); f.probe.available=false; await f.start();
    expect((await f.service.createSession({cwd:'C:\\same',requestId:randomUUID()})).ok).toBe(false);
    expect(f.repository.sessions()).toHaveLength(0);
    const service=new SessionService(f.repository,f.backend,async()=>{throw new Error('Denied');}); f.probe.available=true;
    await service.initialize({userDataDir:'C:\\tmp',helperDir:'C:\\helper',shellScriptDir:'C:\\shell',packagedExecutable:null});
    expect((await service.createSession({cwd:'C:\\same',requestId:randomUUID()})).ok).toBe(false);
    expect(f.repository.sessions()).toHaveLength(0);
  });
  it('does not overwrite registration with a null receipt and keeps new shells unknown until observation', async () => {
    const f=fixture(); f.setReceiptNull(); await f.start(); const s=await f.create();
    expect(s.tabs[0].lifecycle).toBe('open'); expect(s.status).toBe('unknown'); expect(s.canFocus).toBe(true);
    f.observe(s,0); expect(value(f.service.getSnapshot()).sessions[0].status).toBe('waiting');
  });
  it('preserves sticky control failure through polls until explicit clear', async () => {
    const f=fixture(); await f.start(); const s=await f.create(); f.setDenial();
    expect(await f.service.focusSession({sessionId:s.id})).toEqual(failure('FOCUS_DENIED','Switch manually',true));
    f.observe(s,2); expect(value(f.service.getSnapshot()).sessions[0].status).toBe('error');
    const cleared=value(await f.service.clearSessionError({sessionId:s.id})); expect(cleared.status).toBe('running');
  });
  it('requires active/unknown confirmation and keeps observing settled sessions', async () => {
    const f=fixture(); await f.start(); const s=await f.create();
    expect(await f.service.settleSession({sessionId:s.id,confirmActive:false})).toMatchObject({ok:false,error:{code:'SETTLE_CONFIRM_REQUIRED'}});
    f.observe(s,2);
    const settled=value(await f.service.settleSession({sessionId:s.id,confirmActive:true})); expect(settled.status).toBe('settled'); expect(settled.activityStatus).toBe('running');
    expect(value(f.service.getSnapshot()).sessions).toHaveLength(0);
    f.observe(s,0); expect(value(f.service.getHistory({search:'',status:'waiting',page:1,pageSize:20})).total).toBe(1);
    expect((await f.service.addTab({sessionId:s.id})).ok).toBe(false);
    expect(value(await f.service.unsettleSession({sessionId:s.id})).status).toBe('waiting');
  });
  it('registration timeout is uncertain and requires explicit duplicate confirmation', async () => {
    const f=fixture(); f.setRegister(false); await f.start(); const s=await f.create(); const launch=f.launches[0];
    f.emit({type:'operation-error',sessionId:s.id,tabId:s.tabs[0].id,operationId:launch.operationId,error:{code:'REGISTRATION_TIMEOUT',message:'Timed out',retryable:true}});
    expect(value(f.service.getSnapshot()).sessions[0]).toMatchObject({status:'unknown',tabs:[{lifecycle:'launch-uncertain',error:null}]});
    expect(await f.service.retryTab({tabId:s.tabs[0].id,confirmPossibleDuplicate:false})).toMatchObject({ok:false,error:{code:'RETRY_CONFIRM_REQUIRED'}});
    f.setRegister(true); const retried=value(await f.service.retryTab({tabId:s.tabs[0].id,confirmPossibleDuplicate:true})); expect(retried.tabs[0].lifecycle).toBe('open'); expect(f.launches).toHaveLength(2);
    f.emit({type:'operation-error',sessionId:s.id,tabId:s.tabs[0].id,operationId:launch.operationId,error:{code:'LAUNCH_FAILED',message:'Old failure',retryable:true}});
    expect(value(f.service.getSnapshot()).sessions[0].tabs[0].error).toBeNull();
  });
  it('late timeout does not invalidate a registered shell', async () => {
    const f=fixture(); await f.start(); const s=await f.create(); const launch=f.launches[0];
    f.emit({type:'operation-error',sessionId:s.id,tabId:s.tabs[0].id,operationId:launch.operationId,error:{code:'REGISTRATION_TIMEOUT',message:'Late',retryable:true}});
    expect(value(f.service.getSnapshot()).sessions[0].tabs[0].lifecycle).toBe('open');
  });
  it('a pre-spawn failure remains a visible closed error and successful retry clears it', async () => {
    const f=fixture(); f.setError(failure('LAUNCH_FAILED','Spawn failed',true)); await f.start(); const s=await f.create();
    expect(s).toMatchObject({status:'error',tabs:[{lifecycle:'closed',error:{code:'LAUNCH_FAILED'}}]});
    f.setError(undefined); const retried=value(await f.service.retryTab({tabId:s.tabs[0].id,confirmPossibleDuplicate:false})); expect(retried.tabs[0].error).toBeNull(); expect(retried.tabs[0].lifecycle).toBe('open');
  });
  it.each(['lost','rebound'] as const)('refuses append after target is %s during directory validation', async kind => {
    let calls=0; let release!:()=>void; let entered!:()=>void;
    const waiting=new Promise<void>(resolve=>{entered=resolve;});
    const f=fixture(async cwd=>{
      if(++calls===2){entered();await new Promise<void>(resolve=>{release=resolve;});}
      return cwd;
    });
    await f.start(); const s=await f.create();
    const pending=f.service.addTab({sessionId:s.id});
    await waiting;
    f.emit({type:'target-lost',sessionId:s.id,reason:'Destroyed during validation'});
    if(kind==='rebound') f.emit({type:'registered',registration:f.repository.tabs(s.id)[0].registration!,target:{...f.target(f.launches[0]),hwnd:'99999',owner:{pid:101,startTime:'999999'}}});
    release();
    expect(await pending).toMatchObject({ok:false,error:{code:'TARGET_LOST'}});
    expect(f.launches).toHaveLength(1);
    expect(f.repository.tabs(s.id)).toHaveLength(1);
    expect(f.repository.operationMap.size).toBe(1);
  });
  it('revalidates native ownership after the directory await', async () => {
    const f=fixture(); await f.start(); const s=await f.create(); let checks=0;
    f.backend.verifyTarget=async target=>++checks===1?success(target):failure('TARGET_LOST','Window no longer exists',true);
    expect(await f.service.addTab({sessionId:s.id})).toMatchObject({ok:false,error:{code:'TARGET_LOST'}});
    expect(checks).toBe(2); expect(f.launches).toHaveLength(1); expect(f.repository.tabs(s.id)).toHaveLength(1);
  });
  it('never drops the explicit append target if it is lost immediately before dispatch', async () => {
    const f=fixture(); await f.start(); const s=await f.create(); let lost=false;
    const unsubscribe=f.service.subscribe(event=>{
      if(event.reason==='sessions' && !lost && f.repository.tabs(s.id).length===2){
        lost=true;f.emit({type:'target-lost',sessionId:s.id,reason:'Destroyed before dispatch'});
      }
    });
    const result=value(await f.service.addTab({sessionId:s.id}));
    unsubscribe();
    expect(result.tabs[1]).toMatchObject({lifecycle:'closed',error:{code:'TARGET_LOST'}});
    expect(f.launches).toHaveLength(1);
  });
  it('honors a disabled add-tab capability without creating an intent', async () => {
    const f=fixture(); f.probe.capabilities.addTab=false; await f.start(); const s=await f.create();
    expect(s.canAddTab).toBe(false); expect(s.controlReason).toContain('Adding tabs is unavailable');
    expect(await f.service.addTab({sessionId:s.id})).toMatchObject({ok:false,error:{code:'UNSUPPORTED'}});
    expect(f.launches).toHaveLength(1); expect(f.repository.tabs(s.id)).toHaveLength(1); expect(f.repository.operationMap.size).toBe(1);
  });
  it('serializes add-tab ordinals and uses the established target', async () => {
    const f=fixture(); await f.start(); const s=await f.create();
    await Promise.all([f.service.addTab({sessionId:s.id}),f.service.addTab({sessionId:s.id})]);
    expect(f.repository.tabs(s.id).map(t=>t.ordinal)).toEqual([0,1,2]); expect(f.launches.slice(1).every(l=>l.existingTarget?.hwnd==='12345')).toBe(true);
  });
  it('missing observations and helper failure never become healthy waiting', async () => {
    const f=fixture(); await f.start(); const s=await f.create(); f.observe(s,0);
    expect(value(f.service.getSnapshot()).sessions[0].status).toBe('waiting');
    f.emit({type:'observations',items:[]}); expect(value(f.service.getSnapshot()).sessions[0].status).toBe('unknown');
    f.emit({type:'unavailable',error:{code:'NATIVE_UNAVAILABLE',message:'Broker exited',retryable:true}});
    expect(value(f.service.getSnapshot()).sessions[0]).toMatchObject({status:'unknown',canFocus:false,canAddTab:false});
  });
  it('confirmed root exit closes the tab without settling or waiting', async () => {
    const f=fixture(); await f.start(); const s=await f.create(); f.observe(s,0,'exited');
    expect(value(f.service.getSnapshot()).sessions[0]).toMatchObject({status:'unknown',settledAt:null,canAddTab:false,tabs:[{lifecycle:'closed'}]});
  });
  it('target loss does not falsify healthy process tracking', async () => {
    const f=fixture(); await f.start(); const s=await f.create(); f.observe(s,1);
    f.emit({type:'target-lost',sessionId:s.id,reason:'Renamed'});
    expect(value(f.service.getSnapshot()).sessions[0]).toMatchObject({status:'running',canFocus:false,canAddTab:false});
  });
  it('restart never relaunches and turns unfinished intents uncertain', async () => {
    const f=fixture(); f.setRegister(false); await f.start(); const s=await f.create();
    const next=new SessionService(f.repository,f.backend,async cwd=>cwd); await next.initialize({userDataDir:'C:\\tmp',helperDir:'C:\\helper',shellScriptDir:'C:\\shell',packagedExecutable:null});
    expect(f.launches).toHaveLength(1); expect(value(next.getSnapshot()).sessions[0].tabs[0].lifecycle).toBe('launch-uncertain'); expect(s.id).toBe(value(next.getSnapshot()).sessions[0].id);
  });
  it.each(['agents','unknown','uncertain'] as const)('requires confirmation for an error badge hiding %s activity', async activity => {
    const f=fixture(); if(activity==='uncertain')f.setRegister(false);
    await f.start(); const s=await f.create(); const launch=f.launches[0];
    if(activity!=='uncertain')f.observe(s,activity==='agents'?2:0,'alive',activity==='unknown'?'unknown':'healthy');
    f.emit({type:'operation-error',sessionId:s.id,tabId:s.tabs[0].id,operationId:launch.operationId,error:{code:'TARGET_LOST',message:'Sticky failure',retryable:true}});
    expect(value(f.service.getSnapshot()).sessions[0].tabs[0].status).toBe('error');
    expect(await f.service.settleSession({sessionId:s.id,confirmActive:false})).toMatchObject({ok:false,error:{code:'SETTLE_CONFIRM_REQUIRED'}});
    expect(f.repository.session(s.id)?.settledAt).toBeNull();
    expect(value(await f.service.settleSession({sessionId:s.id,confirmActive:true})).status).toBe('settled');
  });
  it('does not require active confirmation for a healthy idle tab with a sticky error', async () => {
    const f=fixture(); await f.start(); const s=await f.create(); f.observe(s,0);
    f.emit({type:'operation-error',sessionId:s.id,tabId:s.tabs[0].id,operationId:f.launches[0].operationId,error:{code:'TARGET_LOST',message:'Sticky failure',retryable:true}});
    expect(value(await f.service.settleSession({sessionId:s.id,confirmActive:false})).status).toBe('settled');
  });
  it.each([false, true])('canonicalizes legacy shell switches with stale executables, including watch changes=%s', async changeRules => {
    const f = fixture();
    const windowsPowerShell = { id: 'windows-powershell' as const, executable: 'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe', available: true, reason: null };
    f.probe.shells.push(windowsPowerShell);
    f.repository.saveSettings({ ...f.repository.settings(), shellId: windowsPowerShell.id, shellExecutable: windowsPowerShell.executable });
    await f.start();
    const initial = f.repository.settings();
    const candidate = { ...initial, shellId: 'pwsh' as const, ...(changeRules ? { processRules: [] } : {}) };
    const first = value(await f.service.saveSettings(candidate));
    expect(first).toEqual({ ...candidate, shellExecutable: f.probe.shells[0].executable });
    expect(f.repository.settings()).toEqual(first);
    const second = value(await f.service.saveSettings({ ...first, shellId: windowsPowerShell.id }));
    expect(second).toEqual({ ...first, shellId: windowsPowerShell.id, shellExecutable: windowsPowerShell.executable });
    expect(f.repository.settings()).toEqual(second);
    await f.service.dispose();
  });
  it('rejects arbitrary legacy executable overrides even when another available shell was selected', async () => {
    const f = fixture(); await f.start();
    const canonical = value(await f.service.saveSettings({ ...f.repository.settings(), shellId: 'pwsh' }));
    f.probe.shells.push({ id: 'windows-powershell', executable: 'C:\\Windows\\powershell.exe', available: true, reason: null });
    const watches = f.watchCount();
    expect(await f.service.saveSettings({ ...canonical, shellId: 'windows-powershell', shellExecutable: 'C:\\arbitrary.exe' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
    expect(f.repository.settings()).toEqual(canonical); expect(f.watchCount()).toBe(watches);
    f.probe.shells[1].available = false;
    expect(await f.service.saveSettings({ ...canonical, shellId: 'windows-powershell' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
    await f.service.dispose();
  });
  it('rejects traversal settings before persistence or native configuration', async () => {
    const f=fixture(); await f.start(); const s=await f.create(); f.observe(s,0);
    const previous=f.repository.settings(); const watches=f.watchCount();
    const candidate={...previous,processRules:[{...previous.processRules[0],scriptPathSuffixes:['..\\agent\\cli.js']}]};
    expect(await f.service.saveSettings(candidate)).toMatchObject({ok:false,error:{code:'VALIDATION'}});
    expect(f.repository.settings()).toEqual(previous); expect(f.watchCount()).toBe(watches);
    expect(value(f.service.getSnapshot()).sessions[0].status).toBe('waiting');
  });
  it('propagates native rule rejection, restores the old watch and discards stale observations', async () => {
    const f=fixture(); await f.start(); const s=await f.create(); f.observe(s,2);
    const previous=f.repository.settings(); const candidate={...previous,accentColor:'#123456',processRules:[{...previous.processRules[0],label:'Rejected native rule'}]};
    const oldTime=new Date().toISOString(); await new Promise(resolve=>setTimeout(resolve,5));
    let applied=previous.processRules; const configurations: SettingsDto['processRules'][]=[];
    f.backend.setWatch=async input=>{
      configurations.push(structuredClone(input.rules));
      f.emit({type:'observations',items:[{sessionId:s.id,tabId:s.tabs[0].id,observedAt:oldTime,root:'alive',health:'healthy',agents:10,reason:null}]});
      if(input.rules[0]?.label==='Rejected native rule')return failure('VALIDATION','Native rule rejected');
      applied=structuredClone(input.rules);return success({configured:true});
    };
    expect(await f.service.saveSettings(candidate)).toEqual(failure('VALIDATION','Native rule rejected'));
    expect(configurations).toEqual([candidate.processRules,previous.processRules]);expect(applied).toEqual(previous.processRules);
    expect(f.repository.settings()).toEqual(previous);
    expect(value(f.service.getSnapshot()).sessions[0]).toMatchObject({status:'unknown',counts:{agents:0}});
    f.emit({type:'observations',items:[{sessionId:s.id,tabId:s.tabs[0].id,observedAt:oldTime,root:'exited',health:'healthy',agents:0,reason:null}]});
    expect(f.repository.tabs(s.id)[0].lifecycle).toBe('open');
    f.observe(s,0);expect(value(f.service.getSnapshot()).sessions[0].status).toBe('waiting');
  });
  it('holds new settings until native acceptance and waits for fresh evidence afterward', async () => {
    const f=fixture(); await f.start(); const s=await f.create(); f.observe(s,3);
    const previous=f.repository.settings(); const candidate={...previous,processRules:[{...previous.processRules[0],enabled:false}]};
    const oldTime=new Date().toISOString();await new Promise(resolve=>setTimeout(resolve,5));
    let release!:()=>void;let entered!:()=>void;const waiting=new Promise<void>(resolve=>{entered=resolve;});
    f.backend.setWatch=async()=>{entered();await new Promise<void>(resolve=>{release=resolve;});return success({configured:true});};
    const pending=f.service.saveSettings(candidate);await waiting;
    expect(f.repository.settings()).toEqual(previous);
    f.observe(s,8);expect(value(f.service.getSnapshot()).sessions[0].status).toBe('unknown');
    const canonical = { ...candidate, shellExecutable: f.probe.shells.find(shell => shell.available && shell.id === candidate.shellId)?.executable ?? candidate.shellExecutable };
    release();expect(value(await pending)).toEqual(canonical);expect(f.repository.settings()).toEqual(canonical);
    f.emit({type:'observations',items:[{sessionId:s.id,tabId:s.tabs[0].id,observedAt:oldTime,root:'alive',health:'healthy',agents:9,reason:null}]});
    expect(value(f.service.getSnapshot()).sessions[0]).toMatchObject({status:'unknown',counts:{agents:0}});
    f.observe(s,0);expect(value(f.service.getSnapshot()).sessions[0].status).toBe('waiting');
  });
  it('restores applied rules when SQLite persistence fails after native acceptance', async () => {
    const f=fixture();await f.start();const s=await f.create();f.observe(s,1);
    const previous=f.repository.settings();const candidate={...previous,accentColor:'#123456',processRules:[{...previous.processRules[0],enabled:false}]};
    let applied=previous.processRules;f.backend.setWatch=async input=>{applied=structuredClone(input.rules);return success({configured:true});};
    f.repository.saveSettings=()=>{throw new Error('Storage failed');};
    expect(await f.service.saveSettings(candidate)).toMatchObject({ok:false,error:{code:'STORAGE_FAILED'}});
    expect(f.repository.settings()).toEqual(previous);expect(applied).toEqual(previous.processRules);
    expect(value(f.service.getSnapshot()).sessions[0].status).toBe('unknown');
    f.observe(s,0);expect(value(f.service.getSnapshot()).sessions[0].status).toBe('waiting');
  });
  it('keeps monitoring unknown when native rejection and rollback both fail', async () => {
    const f=fixture();await f.start();const s=await f.create();f.observe(s,1);
    const previous=f.repository.settings();const candidate={...previous,processRules:[{...previous.processRules[0],enabled:false}]};
    f.backend.setWatch=async()=>failure('MONITOR_UNAVAILABLE','Watch failed',true);
    expect(await f.service.saveSettings(candidate)).toMatchObject({ok:false,error:{code:'MONITOR_UNAVAILABLE'}});
    expect(f.repository.settings()).toEqual(previous);
    f.observe(s,0);expect(value(f.service.getSnapshot()).sessions[0].status).toBe('unknown');
  });
  it('freezes shell settings across asynchronous directory validation', async () => {
    const f=fixture(); let release!:()=>void;
    const service=new SessionService(f.repository,f.backend,async cwd=>{await new Promise<void>(resolve=>{release=resolve;});return cwd;});
    await service.initialize({userDataDir:'C:\\tmp',helperDir:'C:\\helper',shellScriptDir:'C:\\shell',packagedExecutable:null});
    const pending=service.createSession({cwd:'C:\\same',requestId:randomUUID()});
    await new Promise(resolve=>setImmediate(resolve));
    f.repository.saveSettings({...f.repository.settings(),shellId:'windows-powershell'});
    release(); const s=value(await pending);
    expect(s.shellId).toBe('pwsh'); expect(f.launches[0].shellId).toBe('pwsh');
  });
  it('known pre-spawn target failures do not require an uncertain duplicate retry', async () => {
    const f=fixture(); f.setError(failure('TARGET_AMBIGUOUS','Unverified existing window',true)); await f.start(); const s=await f.create();
    expect(s.tabs[0].lifecycle).toBe('closed'); expect(s.status).toBe('error');
  });
  it('click reopens a confirmed closed window on the same session and fences stale loss/close events', async () => {
    const f=fixture(undefined,true);await f.start();const s=await f.create();const oldBinding=f.bindings.get(s.id)!;const oldMember=f.members.get(s.tabs[0].id)!;
    f.closeWindow(s.id);await new Promise(resolve=>setImmediate(resolve));
    expect(value(f.service.getSnapshot()).sessions[0].window).toMatchObject({state:'closed',canReopen:true});
    const reopened=value(await f.service.activateSession({sessionId:s.id}));
    expect(reopened.id).toBe(s.id);expect(f.repository.sessions()).toHaveLength(1);expect(f.reopens).toHaveLength(1);
    expect(f.reopens[0].sessionId).toBe(s.id);expect(f.reopens[0].existingTarget).toBeNull();expect(reopened.tabs).toHaveLength(2);
    expect(reopened.window?.state).toBe('alive');expect(reopened.tabs[1].id).not.toBe(s.tabs[0].id);
    f.windowEvent({type:'lost',binding:oldBinding,reason:'Late old close'});
    f.windowEvent({type:'closed',member:oldMember});f.emit({type:'target-lost',sessionId:s.id,reason:'Unqualified stale close'});
    expect(value(f.service.getSnapshot()).sessions[0].window?.state).toBe('alive');
    expect(f.repository.tabs(s.id)[1].lifecycle).toBe('open');
  });
  it('double click shares one pending same-session replacement and pending receipt cannot cause another', async () => {
    const f=fixture(undefined,true);await f.start();const s=await f.create();f.closeWindow(s.id);
    f.setRegister(false);let release!:()=>void;const ext=f.backend as SessionWindowBackend;
    const reopen=ext.reopenWindow.bind(ext);ext.reopenWindow=async input=>{await new Promise<void>(resolve=>{release=resolve;});return reopen(input);};
    const first=f.service.activateSession({sessionId:s.id}),second=f.service.activateSession({sessionId:s.id});
    expect(second).toBe(first);await new Promise(resolve=>setImmediate(resolve));release();await first;
    expect(f.reopens).toHaveLength(1);expect(f.repository.tabs(s.id)).toHaveLength(2);
    expect(await f.service.activateSession({sessionId:s.id})).toMatchObject({ok:false,error:{code:'RETRY_CONFIRM_REQUIRED'}});
    expect(f.reopens).toHaveLength(1);
  });
  it('an authenticated shell without a verified window binding stays unknown, not closed', async () => {
    const f=fixture(undefined,true);f.setRegister(false);await f.start();const s=await f.create();const input=f.launches[0];
    f.emit({type:'registered',registration:{sessionId:s.id,tabId:s.tabs[0].id,operationId:input.operationId,shell:{pid:200,startTime:'123456789'},shellExecutable:input.shellExecutable,cwd:input.cwd,registeredAt:new Date().toISOString()},target:null});
    expect(value(f.service.getSnapshot()).sessions[0].window).toMatchObject({state:'unknown',canReopen:false,canRegister:false});
    expect((await f.service.activateSession({sessionId:s.id})).ok).toBe(false);expect(f.reopens).toHaveLength(0);
  });
  it('unknown ownership and focus denial never reopen', async () => {
    const f=fixture(undefined,true);await f.start();const s=await f.create();
    f.windowStates.set(s.id,'unavailable');
    expect((await f.service.activateSession({sessionId:s.id})).ok).toBe(false);expect(f.reopens).toHaveLength(0);
    f.windowStates.set(s.id,'alive');f.setDenial();
    expect(await f.service.activateSession({sessionId:s.id})).toMatchObject({ok:false,error:{code:'FOCUS_DENIED'}});
    expect(f.reopens).toHaveLength(0);expect(f.repository.tabs(s.id)).toHaveLength(1);
  });
  it('restart keeps an unfinished same-session replacement uncertain and does not repeat it', async () => {
    const f=fixture(undefined,true);await f.start();const s=await f.create();f.closeWindow(s.id);f.setRegister(false);
    await f.service.activateSession({sessionId:s.id});expect(f.reopens).toHaveLength(1);await f.service.dispose();
    const restored=new SessionService(f.repository,f.backend,async cwd=>cwd);
    await restored.initialize({userDataDir:'C:\\tmp',helperDir:'C:\\helper',shellScriptDir:'C:\\shell',packagedExecutable:null});
    expect(value(restored.getSnapshot()).sessions[0].window?.state).toBe('launch-uncertain');
    expect((await restored.activateSession({sessionId:s.id})).ok).toBe(false);expect(f.reopens).toHaveLength(1);
  });
  it('explicit registration creates no shell until authenticated adoption and safely quotes instructions', async () => {
    const f=fixture(undefined,true);await f.start();const s=await f.create();
    const guide=value(await f.service.prepareSessionRegistration({sessionId:s.id,shellId:'pwsh'}));
    expect(guide.command).toContain("a''b");expect(guide.instructions).toContain('NOT discovered automatically');
    expect(f.launches).toHaveLength(1);expect(f.repository.tabs(s.id)).toHaveLength(1);
    const member=f.adoptManual(0);await new Promise(resolve=>setImmediate(resolve));
    const tab=f.repository.tab(member.registration.tabId)!;
    expect(tab.sessionId).toBe(s.id);expect(tab.lifecycle).toBe('open');expect(f.repository.tabs(s.id)).toHaveLength(2);
    expect(value(f.service.getSnapshot()).sessions[0].tabs[1].status).toBe('unknown');
  });
  it('confirmed membership transfer preserves tab identity and source session, rejecting stale source close/poll', async () => {
    const f=fixture(undefined,true);await f.start();const a=await f.create(),b=await f.create();const previous=f.members.get(a.tabs[0].id)!;
    const originalRequest=[...f.repository.operationMap.values()].find(o=>o.tabId===a.tabs[0].id)!.requestId!;
    value(await f.service.prepareSessionRegistration({sessionId:b.id,shellId:'pwsh'}));const moved=f.adoptManual(0,previous);
    await new Promise(resolve=>setImmediate(resolve));
    expect(moved.registration.tabId).toBe(a.tabs[0].id);expect(f.repository.sessions()).toHaveLength(2);
    expect(f.repository.tabs(a.id)).toHaveLength(0);expect(f.repository.tabs(b.id)).toHaveLength(2);
    expect(f.repository.sessionByRequest(originalRequest)?.id).toBe(a.id);
    f.windowEvent({type:'closed',member:previous});
    f.emit({type:'observations',items:[{sessionId:a.id,tabId:a.tabs[0].id,observedAt:new Date().toISOString(),root:'exited',health:'unknown',agents:0,reason:null}]});
    expect(f.repository.tab(a.tabs[0].id)?.lifecycle).toBe('open');expect(f.repository.tab(a.tabs[0].id)?.sessionId).toBe(b.id);
  });
  it('membership reconciliation recovers a missed authenticated adoption without guessing unregistered tabs', async () => {
    const f=fixture(undefined,true);await f.start();const s=await f.create();
    value(await f.service.prepareSessionRegistration({sessionId:s.id,shellId:'pwsh'}));const member=f.adoptManual(0,null,false);
    expect(f.repository.tabs(s.id)).toHaveLength(1);
    const reconciled=value(await f.service.refreshSessionMembership({sessionId:s.id}));
    expect(reconciled.tabs).toHaveLength(2);expect(reconciled.tabs.some(t=>t.id===member.registration.tabId)).toBe(true);
    expect(f.launches).toHaveLength(1);
  });
  it('restart restores saved window generation and members without replaying a command', async () => {
    const f=fixture(undefined,true);await f.start();const s=await f.create();const binding=f.repository.session(s.id)!.binding!;
    await f.service.dispose();const restored=new SessionService(f.repository,f.backend,async cwd=>cwd);
    await restored.initialize({userDataDir:'C:\\tmp',helperDir:'C:\\helper',shellScriptDir:'C:\\shell',packagedExecutable:null});
    expect(f.repository.session(s.id)?.binding).toEqual(binding);expect(f.launches).toHaveLength(1);
    expect(value(restored.getSnapshot()).sessions[0].window?.state).toBe('alive');
  });
  it('history filters activity while public status remains settled', async () => {
    const f=fixture(); await f.start(); const a=await f.create(); const b=await f.create(); f.observe(a,2); f.observe(b,0);
    // The full batch contains both watched tabs.
    f.emit({type:'observations',items:[{sessionId:a.id,tabId:a.tabs[0].id,observedAt:new Date().toISOString(),root:'alive',health:'healthy',agents:2,reason:null},{sessionId:b.id,tabId:b.tabs[0].id,observedAt:new Date().toISOString(),root:'alive',health:'healthy',agents:0,reason:null}]});
    await f.service.renameSession({sessionId:a.id,title:'100%_literal'}); await f.service.settleSession({sessionId:a.id,confirmActive:true}); await f.service.settleSession({sessionId:b.id,confirmActive:false});
    expect(value(f.service.getHistory({search:'%_',status:'running',page:1,pageSize:1}))).toMatchObject({total:1,items:[{id:a.id,status:'settled',activityStatus:'running'}]});
    expect(value(f.service.getHistory({search:'',status:'all',page:2,pageSize:1})).items).toHaveLength(1);
    expect(value(f.service.getHistory({search:'',status:'settled',page:1,pageSize:20})).total).toBe(2);
  });
});
