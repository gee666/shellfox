import { randomUUID } from 'node:crypto';
import type { NativeProbe, RegistrationGuideDto, Result, SessionWindowDto, ShellId } from '../shared/contracts';
import { failure, success } from '../shared/contracts';
import { resultSchema, launchReceiptSchema } from '../shared/schemas';
import { boundWindowSchema, memberRegistrationSchema, membershipSchema, instructionsSchema, sessionWindowEventSchema, type BoundWindow, type MemberRegistration, type SessionWindowBackend, type SessionWindowEvent } from './platform/session-windows';
import type { LaunchRequest } from '../shared/native-port';
import type { OperationRecord, RepositoryPort, SessionRecord, TabRecord } from './models';
import { validateDirectory } from './directory';
import { z } from 'zod';

interface Host {
  probe(): NativeProbe; changed(): void; verified(id: string, value: boolean): void;
  invalidate(tabId: string): void; watch(): Promise<unknown>;
}
export const sameBinding = (a: BoundWindow | null | undefined, b: BoundWindow | null | undefined): boolean => !!a && !!b && a.generation === b.generation && a.target.sessionId === b.target.sessionId && a.target.hwnd === b.target.hwnd && a.target.owner.pid === b.target.owner.pid && a.target.owner.startTime === b.target.owner.startTime && a.target.markerPrefix === b.target.markerPrefix;
const sameRoot = (a: MemberRegistration, b: MemberRegistration): boolean => a.registration.shell.pid === b.registration.shell.pid && a.registration.shell.startTime === b.registration.shell.startTime;
const sameMember = (a: MemberRegistration | null | undefined, b: MemberRegistration): boolean => !!a && sameRoot(a,b) && a.registration.sessionId === b.registration.sessionId && a.registration.tabId === b.registration.tabId && a.registration.operationId === b.registration.operationId && sameBinding(a.binding,b.binding);

export class SessionWindows {
  private unsubscribe?: () => void;
  constructor(private repository: RepositoryPort, private backend: SessionWindowBackend, private host: Host, private directory = validateDirectory) {}
  subscribe(): void {
    this.unsubscribe = this.backend.subscribeSessionWindows(raw => {
      const event = sessionWindowEventSchema.safeParse(raw);
      if (!event.success) { this.unavailable(); return; }
      try { this.event(event.data); } catch { this.unavailable(); }
    });
  }
  dispose(): void { this.unsubscribe?.(); }
  private async call<T>(action: () => Promise<Result<T>>, schema: z.ZodType<T>): Promise<Result<T>> {
    try { const parsed = resultSchema(schema).safeParse(await action()); return parsed.success ? parsed.data as Result<T> : failure('NATIVE_UNAVAILABLE','Invalid window lifecycle response',true); }
    catch { return failure('NATIVE_UNAVAILABLE','Window lifecycle operation failed',true); }
  }
  unavailable(): void {
    for (const session of this.repository.sessions()) if (session.binding) {
      if (session.windowState !== 'opening' && session.windowState !== 'launch-uncertain') session.windowState = 'unknown';
      this.host.verified(session.id,false); this.repository.saveSession(session);
    }
    this.host.changed();
  }
  async restore(): Promise<void> {
    // Upgrade legacy bindings only from their persisted authenticated initial launch.
    for (const session of this.repository.sessions()) {
      if (!session.binding && session.target) {
        const tab = this.repository.tabs(session.id).find(t => t.registration && this.repository.operation(t.operationId)?.kind === 'create');
        if (tab) { session.binding = {target:session.target,generation:tab.operationId}; this.repository.saveSession(session); }
      }
    }
    const sessions = this.repository.sessions().filter(s => boundWindowSchema.safeParse(s.binding).success);
    const bindings = sessions.map(s => s.binding!);
    for (const tab of this.repository.tabs()) {
      const session = this.repository.session(tab.sessionId);
      if (!tab.member && tab.registration && session?.binding?.generation === tab.operationId) {
        tab.member = {registration:tab.registration,wtSession:null,binding:session.binding};this.repository.saveTab(tab);
      }
    }
    const members = this.repository.tabs().filter(t => t.lifecycle === 'open' && memberRegistrationSchema.safeParse(t.member).success && bindings.some(b => sameBinding(b,t.member!.binding))).map(t => t.member!);
    const result = await this.call(() => this.backend.restoreSessionWindows({bindings,members}), z.object({restored:z.literal(true)}).strict());
    if (!result.ok) { this.unavailable(); return; }
    for (const session of sessions) await this.reconcile(session.id);
  }
  dto(session: SessionRecord): SessionWindowDto {
    const state = session.windowState ?? 'unknown';
    const available = this.host.probe().available;
    return {state,canReopen:available && state === 'closed' && !!session.binding,canRegister:available && state === 'alive' && !!session.binding,
      reason: state === 'closed' ? 'Window closure is confirmed. Click to open a new shell in this same session; old commands are not restored.' : state === 'opening' ? 'Opening a new window. Repeated clicks will not launch another.' : state === 'launch-uncertain' ? 'A replacement may already exist. Inspect Windows Terminal; no click retry will create another.' : state === 'alive' ? 'Only explicitly registered PowerShell tabs are members. Register new or moved tabs in their actual destination.' : 'Window ownership or closure is not confirmed. No replacement will be launched. Refresh membership or inspect Windows Terminal.'};
  }
  private setState(session: SessionRecord, state: SessionRecord['windowState']): void {
    session.windowState = state; session.target = state === 'alive' ? session.binding?.target ?? null : null;
    this.host.verified(session.id,state === 'alive'); this.repository.saveSession(session);
  }
  event(event: SessionWindowEvent): void {
    let changed = false;
    this.repository.transaction(() => {
      if (event.type === 'bound') {
        const session = this.repository.session(event.binding.target.sessionId);
        const operation = this.repository.operation(event.binding.generation);
        const tab = operation?.tabId ? this.repository.tab(operation.tabId) : undefined;
        if (!session || !operation || !tab || operation.sessionId !== session.id || tab.operationId !== operation.id || !['create','reopen'].includes(operation.kind) || tab.lifecycle === 'closed') return;
        if (session.binding && !sameBinding(session.binding,event.binding) && operation.state === 'registered') return;
        session.binding = event.binding; this.setState(session,'alive'); changed = true;
      } else if (event.type === 'lost') {
        const session = this.repository.session(event.binding.target.sessionId);
        if (!session || !sameBinding(session.binding,event.binding) || session.windowState === 'opening' || session.windowState === 'launch-uncertain') return;
        this.setState(session,'closed'); changed = true;
      } else if (event.type === 'closed') {
        const tab = this.repository.tab(event.member.registration.tabId);
        if (!tab || !sameMember(tab.member,event.member)) return;
        tab.lifecycle = 'closed'; this.repository.saveTab(tab); this.host.invalidate(tab.id); changed = true;
      } else changed = this.adopt(event.member,event.previous,event.source);
    });
    if (changed) { void this.host.watch(); this.host.changed(); }
  }
  private adopt(member: MemberRegistration, previous: MemberRegistration | null, source: 'launch' | 'manual' | 'reconcile'): boolean {
    const r = member.registration;
    const session = this.repository.session(r.sessionId);
    if (!session || !sameBinding(session.binding,member.binding)) return false;
    const operation = this.repository.operation(r.operationId);
    let tab = this.repository.tab(r.tabId);
    if (tab?.member && sameMember(tab.member,member)) return false;
    if (!operation || operation.sessionId !== session.id || (source === 'manual' && operation.kind !== 'adopt')) return false;
    if (tab && tab.operationId !== r.operationId && operation.state === 'registered') return false;
    if (source === 'reconcile' && !previous && tab?.member && sameRoot(tab.member,member)) previous = tab.member;
    const rootOwner = this.repository.tabs().find(t => t.lifecycle !== 'closed' && t.registration?.shell.pid === r.shell.pid && t.registration.shell.startTime === r.shell.startTime);
    if (rootOwner && rootOwner.id !== r.tabId) return false;
    if (previous) {
      if (!tab || !sameMember(tab.member,previous) || !sameRoot(previous,member)) return false;
    } else if (tab && (tab.sessionId !== session.id || tab.operationId !== r.operationId)) return false;
    if (!tab) {
      if (operation.kind !== 'adopt') return false;
      tab = {id:r.tabId,sessionId:session.id,title:'Registered shell',cwd:r.cwd,ordinal:0,createdAt:r.registeredAt,lifecycle:'open',operationId:r.operationId,registration:r,error:null};
    }
    const moved = tab.sessionId !== session.id;
    const ordinal = moved || !this.repository.tab(tab.id) ? this.nextOrdinal(session.id) : tab.ordinal;
    tab = {...tab,sessionId:session.id,ordinal,cwd:r.cwd,lifecycle:'open',operationId:r.operationId,registration:r,member,error:null};
    this.repository.saveTab(tab);
    if (moved) for (const old of this.repository.operationsForTab(tab.id)) {
      // Creation delivery belongs to its original session even if its shell is later moved.
      this.repository.saveOperation(old.requestId ? {...old,tabId:null} : {...old,sessionId:session.id});
    }
    this.repository.saveOperation({...operation,tabId:tab.id,state:'registered',updatedAt:new Date().toISOString(),error:null});
    this.host.invalidate(tab.id);
    session.updatedAt = new Date().toISOString(); this.repository.saveSession(session);
    return true;
  }
  private nextOrdinal(sessionId: string): number { return this.repository.tabs(sessionId).reduce((n,t) => Math.max(n,t.ordinal),-1)+1; }
  async reconcile(sessionId: string): Promise<Result<SessionRecord>> {
    const session = this.repository.session(sessionId);
    if (!session) return failure('NOT_FOUND','Session not found');
    if (!session.binding) return failure('TARGET_LOST','No authenticated window generation is saved. Inspect the native terminal.',true);
    const binding = session.binding;
    const result = await this.call(() => this.backend.getWindowMembership(binding), membershipSchema);
    const current = this.repository.session(sessionId)!;
    if (!sameBinding(current.binding,binding)) return failure('TARGET_LOST','Window generation changed during membership refresh',true);
    if (!result.ok || !sameBinding(result.value.binding,binding)) {
      if (current.windowState !== 'opening' && current.windowState !== 'launch-uncertain') this.setState(current,'unknown');
      this.host.changed(); return result.ok ? failure('TARGET_LOST','Membership returned a different window generation',true) : result;
    }
    if (current.windowState !== 'opening' && current.windowState !== 'launch-uncertain') this.setState(current,result.value.windowState === 'unavailable' ? 'unknown' : result.value.windowState);
    let changed = false;
    this.repository.transaction(() => {
      for (const member of result.value.members) changed = this.adopt(member,null,'reconcile') || changed;
      const supplied = new Set(result.value.members.map(m => m.registration.tabId));
      for (const tab of this.repository.tabs(sessionId)) if (tab.member && !supplied.has(tab.id)) this.host.invalidate(tab.id);
    });
    if (changed) await this.host.watch();
    this.host.changed(); return success(this.repository.session(sessionId)!);
  }
  async focus(sessionId: string): Promise<Result<{focused:true}>> {
    const result = await this.reconcile(sessionId);
    if (!result.ok) return result;
    if (result.value.windowState !== 'alive' || !result.value.binding) return failure('TARGET_LOST','The window is not confirmed alive. Focus did not launch a replacement.',true);
    return this.call(() => this.backend.focusSessionWindow(result.value.binding!),z.object({focused:z.literal(true)}).strict());
  }
  private launchRequest(session: SessionRecord, tabId: string, operationId: string, shellId = session.shellId, shellExecutable = session.shellExecutable): LaunchRequest {
    return {sessionId:session.id,tabId,operationId,cwd:session.cwd,shellId,shellExecutable,windowName:`shellfox-${session.id}`,titleMarker:`SHELLFOX:${session.id}:${tabId}`,existingTarget:null};
  }
  async activate(sessionId: string): Promise<Result<SessionRecord>> {
    const checked = await this.reconcile(sessionId);
    if (!checked.ok) return checked;
    const session = checked.value;
    if (session.windowState === 'alive') {
      const focused = await this.call(() => this.backend.focusSessionWindow(session.binding!),z.object({focused:z.literal(true)}).strict());
      if (!focused.ok) return focused;
      return success(session);
    }
    if (session.windowState !== 'closed' || !session.binding) return failure('RETRY_CONFIRM_REQUIRED','Closure is not confirmed, or a replacement is already pending. Inspect Windows Terminal; no new window was launched.');
    if (!this.host.probe().available || !this.host.probe().capabilities.createWindow) return failure('UNSUPPORTED','Native window creation is unavailable');
    try { await this.directory(session.cwd); } catch { return failure('VALIDATION','The session default directory is not accessible'); }
    const current = this.repository.session(sessionId)!;
    if (!sameBinding(current.binding,session.binding) || current.windowState !== 'closed') return failure('TARGET_LOST','Window ownership changed before replacement',true);
    const now = new Date().toISOString();
    const tab: TabRecord = {id:randomUUID(),sessionId,title:`Shell ${this.nextOrdinal(sessionId)+1}`,cwd:session.cwd,ordinal:this.nextOrdinal(sessionId),createdAt:now,lifecycle:'launching',operationId:randomUUID(),registration:null,error:null};
    const operation: OperationRecord = {id:tab.operationId,sessionId,tabId:tab.id,requestId:null,kind:'reopen',state:'intent',createdAt:now,updatedAt:now,error:null};
    this.repository.transaction(() => { current.windowState='opening';this.repository.saveSession(current);this.repository.saveTab(tab);this.repository.saveOperation(operation); });
    this.host.changed();
    const receipt = await this.call(() => this.backend.reopenWindow({request:this.launchRequest(session,tab.id,tab.operationId),previous:session.binding!}),launchReceiptSchema);
    this.repository.transaction(() => {
      const latest = this.repository.tab(tab.id)!;
      if (latest.registration) return; // Registration can arrive before the receipt.
      if (!receipt.ok || receipt.value.operationId !== tab.operationId) {
        const error = receipt.ok ? {code:'NATIVE_UNAVAILABLE' as const,message:'Replacement reply did not match the operation',retryable:true} : receipt.error;
        latest.lifecycle='launch-uncertain';latest.error=error;this.repository.saveTab(latest);
        this.repository.saveOperation({...operation,state:'uncertain',error});
        const s=this.repository.session(sessionId)!;s.windowState='launch-uncertain';this.repository.saveSession(s);
      } else this.repository.saveOperation({...operation,state:'dispatched'});
    });
    this.host.changed(); return success(this.repository.session(sessionId)!);
  }
  async prepare(sessionId: string, shellId: ShellId): Promise<Result<RegistrationGuideDto>> {
    const checked=await this.reconcile(sessionId);
    if (!checked.ok) return checked;
    const session=checked.value;
    if (session.windowState !== 'alive' || !session.binding) return failure('TARGET_LOST','Select a confirmed live destination window before registering a terminal');
    const shell=this.host.probe().shells.find(s=>s.id===shellId && s.available);
    if (!shell) return failure('DEPENDENCY_MISSING','The selected supported PowerShell is unavailable');
    const operationId=randomUUID(),tabId=randomUUID(),now=new Date().toISOString();
    const operation: OperationRecord={id:operationId,sessionId,tabId:null,requestId:null,kind:'adopt',state:'intent',createdAt:now,updatedAt:now,error:null};
    this.repository.saveOperation(operation);
    const result=await this.call(()=>this.backend.prepareRegistration({request:this.launchRequest(session,tabId,operationId,shellId,shell.executable),binding:session.binding!}),instructionsSchema);
    if (!result.ok) { this.repository.saveOperation({...operation,state:'failed',error:result.error});return result; }
    const quote=(s:string)=>"'"+s.replaceAll("'","''")+"'";
    return success({command:`& ${quote(result.value.scriptPath)} -TicketPath ${quote(result.value.ticketPath)}`,expiresAt:result.value.expiresAt,titleMarker:result.value.titleMarker,
      instructions:'Run this command in the actual selected PowerShell tab in this session window. New or moved tabs are NOT discovered automatically. If WT suppresses titles, Rename Tab to the supplied marker first. Preparing a command does not adopt or launch anything. A successful registration can transfer a shell from its previous session.'});
  }
  operationError(tab: TabRecord): void {
    if (tab.registration) return;
    const op=this.repository.operation(tab.operationId);
    if (op?.kind !== 'reopen' && op?.kind !== 'create') return;
    const session=this.repository.session(tab.sessionId);
    if (session && (session.windowState==='opening' || !session.binding)) {session.windowState='launch-uncertain';this.repository.saveSession(session);}
  }
}
