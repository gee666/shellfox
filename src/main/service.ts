import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { z } from 'zod';
import type { AppError, ChangedEvent, EnvVar, ExplorerIntegrationDto, HistoryPage, HistoryQuery, ManagerSnapshot, NativeProbe, Result, SessionDto, SettingsDto, TabDto } from '../shared/contracts';
import { failure, success } from '../shared/contracts';
import type { LaunchReceipt, NativeBackend, NativeEvent, NativeInit, TabObservation, WindowTarget } from '../shared/native-port';
import { explorerSchema, launchReceiptSchema, nativeEventSchema, probeSchema, requestSchemas, resultSchema, windowTargetSchema } from '../shared/schemas';
import { aggregateStatus, countTabs, needsSettleConfirmation, publicStatus, sortSessions, tabStatus } from '../shared/status';
import type { OperationRecord, RepositoryPort, SessionRecord, TabRecord } from './models';
import { unavailableExplorer, unavailableProbe, unavailableCli } from './defaults';
import { validateDirectory } from './directory';
import { hasSessionWindowBackend } from './platform/session-windows';
import { SessionWindows } from './session-windows';

export class SessionService {
  revision = 0;
  probe: NativeProbe = unavailableProbe('Native backend has not initialized');
  cli = { ...unavailableCli, reason: 'Shellfox CLI integration requires embedded terminals.' };
  explorer: ExplorerIntegrationDto = unavailableExplorer;
  private observations = new Map<string, TabObservation>();
  private verified = new Set<string>();
  private listeners = new Set<(event: ChangedEvent) => void>();
  private queues = new Map<string, Promise<unknown>>();
  private requests = new Map<string, Promise<Result<SessionDto>>>();
  private watchQueue: Promise<void> = Promise.resolve();
  private watchUpdating = false;
  private watchReady = false;
  private observationFloor: string | null = null;
  private unsubscribe: (() => void) | undefined;
  private disposed = false;
  private windows?: SessionWindows;
  private activations = new Map<string, Promise<Result<SessionDto>>>();
  constructor(readonly repository: RepositoryPort, readonly backend: NativeBackend, private directoryValidator = validateDirectory, private packagedExecutable: string | null = null) {
    if (hasSessionWindowBackend(backend)) this.windows = new SessionWindows(repository,backend,{
      probe:()=>this.probe,changed:()=>this.changed('native'),verified:(id,value)=>{if(value)this.verified.add(id);else this.verified.delete(id);},
      invalidate:id=>this.observations.delete(id),watch:()=>this.refreshWatch(),
    },directoryValidator);
  }

  subscribe(listener: (event: ChangedEvent) => void): () => void { this.listeners.add(listener); return () => this.listeners.delete(listener); }
  selectSession(sessionId: string): void { this.changed('sessions', sessionId); }
  private changed(reason: ChangedEvent['reason'] = 'sessions', selectSessionId?: string): void {
    const event: ChangedEvent = { revision: ++this.revision, reason, ...(selectSessionId ? { selectSessionId } : {}) };
    for (const listener of this.listeners) { try { listener(event); } catch { /* A disconnected renderer cannot break persistence. */ } }
  }
  private async native<T>(action: () => Promise<Result<T>>, schema: z.ZodType<T>): Promise<Result<T>> {
    try {
      const result = resultSchema(schema).safeParse(await action());
      return result.success ? result.data as Result<T> : failure('NATIVE_UNAVAILABLE', 'Native backend returned an invalid response', true);
    } catch { return failure('NATIVE_UNAVAILABLE', 'Native backend request failed', true); }
  }
  async initialize(input: NativeInit): Promise<void> {
    this.windows?.subscribe();
    this.unsubscribe = this.backend.subscribe(event => {
      try {
        const parsed = nativeEventSchema.safeParse(event);
        this.applyEvent(parsed.success ? parsed.data : { type: 'unavailable', error: { code: 'NATIVE_UNAVAILABLE', message: 'Invalid native event', retryable: true } });
      } catch { this.monitorUnavailable('A native update could not be saved'); }
    });
    const result = await this.native(() => this.backend.initialize(input), probeSchema);
    this.probe = result.ok ? result.value : unavailableProbe(result.error.message);
    const integration = await this.native(() => this.packagedExecutable && this.repository.explorerPreference()
      ? this.backend.setExplorerIntegration({ installed: true, executablePath: this.packagedExecutable })
      : this.backend.getExplorerIntegration(), explorerSchema);
    this.explorer = integration.ok ? integration.value : { ...unavailableExplorer, reason: integration.error.message };
    this.repository.transaction(() => {
      for (const session of this.repository.sessions()) if (session.windowState === 'opening') {
        session.windowState = 'launch-uncertain';this.repository.saveSession(session);
      }
      for (const tab of this.repository.tabs()) {
        if (tab.lifecycle === 'launching') {
          tab.lifecycle = 'launch-uncertain';
          this.repository.saveTab(tab);
          this.updateOperation(tab.operationId, 'uncertain', null);
        }
      }
    });
    if (this.windows) await this.windows.restore();
    await this.refreshWatch();
    for (const session of this.repository.sessions()) {
      if (!session.binding && session.target && this.probe.available) await this.verify(session);
    }
    this.changed('native');
  }
  async dispose(): Promise<void> { this.disposed = true; this.windows?.dispose(); this.unsubscribe?.(); await this.watchQueue; await this.backend.dispose(); }
  private serialize<T>(id: string, action: () => Promise<Result<T>>): Promise<Result<T>> {
    const previous = this.queues.get(id) || Promise.resolve();
    const next = previous.catch(() => undefined).then(action).catch(() => failure('STORAGE_FAILED', 'The operation could not be saved', true)) as Promise<Result<T>>;
    this.queues.set(id, next);
    void next.finally(() => { if (this.queues.get(id) === next) this.queues.delete(id); });
    return next;
  }
  private monitorUnavailable(reason: string): void {
    this.windows?.unavailable();
    this.probe = unavailableProbe(reason);
    this.watchReady = false;
    this.verified.clear();
    for (const tab of this.repository.tabs()) if (tab.lifecycle === 'open') this.observations.delete(tab.id);
    this.changed('native');
  }
  private queueWatch<T>(action: () => Promise<T>): Promise<T> {
    const next = this.watchQueue.then(async () => {
      // Never show badges from the preceding rule set or an unacknowledged replacement.
      this.watchUpdating = true;
      this.watchReady = false;
      this.observations.clear();
      try { return await action(); }
      finally { this.watchUpdating = false; this.changed('native'); }
    });
    this.watchQueue = next.then(() => undefined, () => undefined);
    return next;
  }
  private async replaceWatch(rules: SettingsDto['processRules']): Promise<Result<{ configured: true }>> {
    if (this.disposed) return failure('NATIVE_UNAVAILABLE', 'The manager is shutting down');
    const tabs = this.repository.tabs().filter(t => t.registration && t.lifecycle === 'open').map(t => ({ sessionId: t.sessionId, tabId: t.id, registration: t.registration! }));
    const response = await this.native(() => this.backend.setWatch({ tabs, rules }), z.object({ configured: z.literal(true) }).strict());
    this.watchReady = response.ok;
    if (response.ok) this.observationFloor = new Date().toISOString();
    return response;
  }
  private refreshWatch(): Promise<Result<{ configured: true }>> {
    return this.queueWatch(() => this.replaceWatch(this.repository.settings().processRules));
  }
  private ownsTarget(sessionId: string, expected: WindowTarget): boolean {
    const current = this.repository.session(sessionId)?.target;
    return this.verified.has(sessionId) && !!current && current.sessionId === expected.sessionId &&
      current.hwnd === expected.hwnd && current.owner.pid === expected.owner.pid &&
      current.owner.startTime === expected.owner.startTime && current.windowName === expected.windowName &&
      current.markerPrefix === expected.markerPrefix && current.verification === expected.verification;
  }
  private targetValid(target: WindowTarget, sessionId: string): boolean {
    return target.sessionId === sessionId && target.windowName === `shellfox-${sessionId}` && target.markerPrefix === `SHELLFOX:${sessionId}:`;
  }
  private bind(session: SessionRecord, target: WindowTarget | null): void {
    if (!target || !this.targetValid(target, session.id)) return;
    // A receipt cannot silently replace an established HWND after an add-tab race.
    if (session.target && this.verified.has(session.id) && session.target.hwnd !== target.hwnd) return;
    session.target = target;
    this.verified.add(session.id);
    this.repository.saveSession(session);
  }
  private async verify(session: SessionRecord): Promise<Result<WindowTarget>> {
    if (!session.target || !this.probe.available) return failure('TARGET_LOST', 'No verified native window is available', true);
    const previous = session.target;
    const response = await this.native(() => this.backend.verifyTarget(previous), windowTargetSchema);
    const current = this.repository.session(session.id);
    if (!current || current.target?.hwnd !== previous.hwnd) return failure('TARGET_LOST', 'Native target changed during verification', true);
    if (!response.ok || !this.targetValid(response.value, session.id) || response.value.hwnd !== previous.hwnd || response.value.owner.pid !== previous.owner.pid || response.value.owner.startTime !== previous.owner.startTime) {
      this.verified.delete(session.id);
      current.target = null;
      this.repository.saveSession(current);
      return response.ok ? failure('TARGET_LOST', 'Native window ownership changed', true) : response;
    }
    this.bind(current, response.value);
    return response;
  }
  private updateOperation(id: string, state: OperationRecord['state'], error: AppError | null): void {
    const operation = this.repository.operation(id);
    if (operation) this.repository.saveOperation({ ...operation, state, error, updatedAt: new Date().toISOString() });
  }
  private operationError(tab: TabRecord, error: AppError, preSpawn = false): void {
    if (error.code === 'REGISTRATION_TIMEOUT') {
      if (tab.registration || tab.lifecycle === 'open' || tab.lifecycle === 'closed') return;
      tab.lifecycle = 'launch-uncertain';
      this.updateOperation(tab.operationId, 'uncertain', error);
    } else {
      tab.error = error;
      // Only a confirmed launch failure closes an unregistered intent. Transport failure is uncertain.
      if (!tab.registration) tab.lifecycle = error.code === 'LAUNCH_FAILED' || error.code === 'VALIDATION' || error.code === 'UNSUPPORTED' || error.code === 'DEPENDENCY_MISSING' || (preSpawn && ['TARGET_LOST', 'TARGET_AMBIGUOUS', 'AUTH_FAILED'].includes(error.code)) ? 'closed' : 'launch-uncertain';
      this.updateOperation(tab.operationId, 'failed', error);
    }
    this.repository.saveTab(tab);
    this.windows?.operationError(tab);
  }
  applyEvent(event: NativeEvent): void {
    if (this.disposed) return;
    let watchChanged = false;
    this.repository.transaction(() => {
      switch (event.type) {
        case 'registered': {
          const r = event.registration;
          const tab = this.repository.tab(r.tabId);
          const session = this.repository.session(r.sessionId);
          if (!tab || !session || tab.sessionId !== r.sessionId || tab.operationId !== r.operationId || tab.lifecycle === 'closed') return;
          if (r.cwd !== tab.cwd || r.shellExecutable.toLowerCase() !== session.shellExecutable.toLowerCase()) return;
          tab.registration = r; tab.lifecycle = 'open'; tab.error = null;
          this.repository.saveTab(tab);
          this.updateOperation(r.operationId, 'registered', null);
          if (this.windows && session.binding?.generation !== r.operationId && ['create','reopen'].includes(this.repository.operation(r.operationId)?.kind ?? '')) {
            session.windowState = session.binding ? 'launch-uncertain' : 'unknown';this.repository.saveSession(session);
          } else this.bind(session, event.target);
          watchChanged = true;
          break;
        }
        case 'observations': {
          if (this.watchUpdating || !this.watchReady) return;
          const supplied = new Set<string>();
          for (const item of event.items) {
            const tab = this.repository.tab(item.tabId);
            if (!tab || !tab.registration || tab.sessionId !== item.sessionId || tab.lifecycle !== 'open' || item.observedAt < tab.registration.registeredAt || (this.observationFloor && item.observedAt < this.observationFloor)) continue;
            supplied.add(item.tabId);
            const previous = this.observations.get(item.tabId);
            if (previous && previous.observedAt > item.observedAt) continue;
            this.observations.set(item.tabId, item);
            if (item.root === 'exited') {
              tab.lifecycle = 'closed'; this.repository.saveTab(tab); watchChanged = true;
              if (!this.repository.session(tab.sessionId)?.binding && !this.repository.tabs(tab.sessionId).some(t => t.lifecycle === 'open' || t.lifecycle === 'launching')) this.verified.delete(tab.sessionId);
            }
          }
          for (const tab of this.repository.tabs()) if (tab.lifecycle === 'open' && tab.registration && !supplied.has(tab.id)) this.observations.delete(tab.id);
          break;
        }
        case 'target-lost': {
          const session = this.repository.session(event.sessionId);
          if (session && !session.binding) { session.target = null; this.verified.delete(session.id); this.repository.saveSession(session); }
          break;
        }
        case 'operation-error': {
          const tab = this.repository.tab(event.tabId);
          if (tab && tab.sessionId === event.sessionId && tab.operationId === event.operationId) this.operationError(tab, event.error);
          break;
        }
        case 'unavailable': this.monitorUnavailable(event.error.message); break;
      }
    });
    if (watchChanged) void this.refreshWatch();
    this.changed('native');
  }
  toDto(session: SessionRecord): SessionDto {
    const tabs: TabDto[] = this.repository.tabs(session.id).map(tab => {
      const observation = this.observations.get(tab.id);
      const status = tabStatus(tab.lifecycle, tab.error, observation);
      return { id: tab.id, sessionId: tab.sessionId, title: tab.title, cwd: tab.cwd, ordinal: tab.ordinal, createdAt: tab.createdAt, lifecycle: tab.lifecycle, status, agents: tab.lifecycle === 'open' && observation?.root === 'alive' ? observation.agents : 0, monitoringReason: tab.lifecycle === 'closed' ? 'Shell exited or launch failed' : observation?.reason ?? (tab.lifecycle === 'launch-uncertain' ? 'Launch was not confirmed. Another window may already exist.' : status === 'unknown' ? 'Awaiting verified shell observation' : null), error: tab.error };
    });
    const activityStatus = aggregateStatus(tabs, session.error);
    const bound = this.probe.available && this.verified.has(session.id) && !!session.target && (!this.windows || (!!session.binding && session.windowState === 'alive'));
    const canFocus = bound && this.probe.capabilities.focusWindow;
    const canAddTab = bound && this.probe.capabilities.addTab && !session.settledAt && tabs.some(t => t.lifecycle === 'open');
    return { window: this.windows ? this.windows.dto(session) : {state:'unsupported',canReopen:false,canRegister:false,reason:'This backend does not support authenticated window lifecycle or explicit terminal registration.'}, id: session.id, title: session.title, cwd: session.cwd, adapterId: session.adapterId, shellId: session.shellId, createdAt: session.createdAt, updatedAt: session.updatedAt, settledAt: session.settledAt, status: publicStatus(activityStatus, session.settledAt), activityStatus, counts: countTabs(tabs), tabs, env: session.env ?? [], canFocus, canAddTab, controlReason: !tabs.some(t => t.lifecycle !== 'closed') ? 'No open terminals' : !bound ? 'No verified native window. Select the terminal manually.' : !this.probe.capabilities.addTab ? 'Adding tabs is unavailable for this backend. Select individual tabs in Windows Terminal.' : session.settledAt ? 'Unsettle before adding a tab. Select individual tabs in Windows Terminal.' : 'Select individual tabs in Windows Terminal.', error: session.error };
  }
  getSnapshot(): Result<ManagerSnapshot> { return success({ revision: this.revision, sessions: sortSessions(this.repository.sessions().filter(s => !s.settledAt).map(s => this.toDto(s))), settings: this.repository.settings(), probe: this.probe, explorer: this.explorer, cli: this.cli }); }
  getHistory(input: HistoryQuery): Result<HistoryPage> {
    const query = requestSchemas.getHistory.safeParse(input);
    if (!query.success) return failure('VALIDATION', 'Invalid history query');
    const { search, status, page, pageSize } = query.data;
    const items = this.repository.settled(search).map(s => this.toDto(s)).filter(s => status === 'all' || status === 'settled' || s.activityStatus === status);
    return success({ items: items.slice((page - 1) * pageSize, page * pageSize), total: items.length, page, pageSize });
  }
  private shell(settings = this.repository.settings()): Result<string> {
    if (!this.probe.available || !this.probe.capabilities.createWindow) return failure('UNSUPPORTED', this.probe.reasons[0] || 'Native launches are unavailable');
    const option = this.probe.shells.find(s => s.id === settings.shellId && s.available && (!settings.shellExecutable || path.normalize(s.executable).toLowerCase() === path.normalize(settings.shellExecutable).toLowerCase()));
    return option ? success(option.executable) : failure('DEPENDENCY_MISSING', 'The selected supported shell executable is unavailable');
  }
  createSession(input: { cwd: string; requestId: string; title?: string }): Promise<Result<SessionDto>> {
    const parsed = requestSchemas.createSession.safeParse(input);
    if (!parsed.success) return Promise.resolve(failure('VALIDATION', 'Invalid session request'));
    const existing = this.requests.get(input.requestId);
    if (existing) return existing.then(r => r.ok ? success(this.toDto(this.repository.session(r.value.id)!)) : r);
    const request = this.serialize('create', async () => {
      const persisted = this.repository.sessionByRequest(input.requestId);
      if (persisted) return success(this.toDto(persisted));
      const settings = this.repository.settings();
      const shell = this.shell(settings);
      if (!shell.ok) return shell;
      if (this.repository.tabs().filter(t => t.lifecycle !== 'closed').length >= 1000 || this.repository.sessions().filter(s => !s.settledAt).length >= 10000) return failure('VALIDATION', 'The managed session or tab limit has been reached');
      let cwd: string;
      try { cwd = await this.directoryValidator(input.cwd); } catch { return failure('VALIDATION', 'Choose an accessible absolute local directory'); }
      const now = new Date().toISOString();
      const session: SessionRecord = { id: randomUUID(), title: input.title ?? (path.basename(cwd).slice(0, 200) || cwd.slice(0, 200)), cwd, adapterId: settings.adapterId, shellId: settings.shellId, shellExecutable: shell.value, createdAt: now, updatedAt: now, settledAt: null, error: null, target: null };
      if (this.windows) { session.binding = null; session.windowState = 'opening'; }
      const tab = this.intent(session, 'Shell 1', 0);
      this.repository.transaction(() => { this.repository.saveSession(session); this.repository.saveTab(tab); this.saveIntent(tab, 'create', input.requestId); });
      this.changed();
      await this.dispatch(session.id, tab.id);
      return success(this.toDto(this.repository.session(session.id)!));
    });
    this.requests.set(input.requestId, request);
    void request.then(result => { if (!result.ok) this.requests.delete(input.requestId); });
    return request;
  }
  private intent(session: SessionRecord, title: string, ordinal: number): TabRecord {
    return { id: randomUUID(), sessionId: session.id, title, cwd: session.cwd, ordinal, createdAt: new Date().toISOString(), lifecycle: 'launching', operationId: randomUUID(), registration: null, error: null };
  }
  private saveIntent(tab: TabRecord, kind: OperationRecord['kind'], requestId: string | null = null): void {
    const now = new Date().toISOString();
    this.repository.saveOperation({ id: tab.operationId, tabId: tab.id, sessionId: tab.sessionId, kind, requestId, state: 'intent', createdAt: now, updatedAt: now, error: null });
  }
  private async dispatch(sessionId: string, tabId: string, expectedTarget?: WindowTarget): Promise<void> {
    const session = this.repository.session(sessionId)!;
    const tab = this.repository.tab(tabId)!;
    const operationId = tab.operationId;
    if (expectedTarget && !this.ownsTarget(sessionId, expectedTarget)) {
      this.repository.transaction(() => this.operationError(tab, { code: 'TARGET_LOST', message: 'Native window ownership was lost before dispatch. No replacement was launched.', retryable: true }, true));
      this.changed();
      return;
    }
    const response: Result<LaunchReceipt> = await this.native(() => this.backend.launch({ sessionId, tabId, operationId, cwd: tab.cwd, shellId: session.shellId, shellExecutable: session.shellExecutable, windowName: `shellfox-${sessionId}`, titleMarker: `SHELLFOX:${sessionId}:${tabId}`, existingTarget: expectedTarget ?? (this.verified.has(sessionId) ? session.target : null) }), launchReceiptSchema);
    this.repository.transaction(() => {
      const current = this.repository.tab(tabId);
      if (!current || current.operationId !== operationId) return;
      if (!response.ok) { if (!current.registration) this.operationError(current, response.error, true); return; }
      if (response.value.operationId !== operationId) { if (!current.registration) this.operationError(current, { code: 'NATIVE_UNAVAILABLE', message: 'Launch reply did not match the operation', retryable: true }); return; }
      if (current.lifecycle !== 'launching' && current.lifecycle !== 'open') return;
      if (current.lifecycle === 'launching') this.updateOperation(operationId, 'dispatched', null);
      const latestSession = this.repository.session(sessionId)!;
      // Null receipt targets must not overwrite registration events delivered during launch.
      if (!latestSession.target) this.bind(latestSession, response.value.target);
    });
    this.changed();
  }
  addTab(input: { sessionId: string; title?: string }): Promise<Result<SessionDto>> {
    if (!requestSchemas.addTab.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid tab request'));
    return this.serialize(input.sessionId, async () => {
      const session = this.repository.session(input.sessionId);
      if (!session) return failure('NOT_FOUND', 'Session not found');
      if (!this.probe.capabilities.addTab) return failure('UNSUPPORTED', 'Adding tabs is disabled by this backend. Create a separate session instead.');
      if (!this.toDto(session).canAddTab) return failure('UNSUPPORTED', 'Unsettle and verify a live native window before adding a tab');
      const verification = await this.verify(session);
      if (!verification.ok) { this.stickyControl(session.id, verification.error); return verification; }
      try { await this.directoryValidator(session.cwd); } catch { return failure('VALIDATION', 'The default directory is no longer accessible'); }
      const expectedTarget = verification.value;
      if (!this.ownsTarget(session.id, expectedTarget)) {
        const lost = failure('TARGET_LOST', 'Native window ownership was lost during directory validation. No replacement was launched.', true);
        if (!lost.ok) this.stickyControl(session.id, lost.error);
        return lost;
      }
      const currentSession = this.repository.session(session.id)!;
      const revalidated = await this.verify(currentSession);
      if (!revalidated.ok) { this.stickyControl(session.id, revalidated.error); return revalidated; }
      if (!this.ownsTarget(session.id, expectedTarget)) return failure('TARGET_LOST', 'Native window ownership changed before adding a tab', true);
      if (!this.toDto(this.repository.session(session.id)!).canAddTab) return failure('UNSUPPORTED', 'Adding tabs is no longer available');
      const tabs = this.repository.tabs(session.id);
      if (tabs.length >= 1000 || this.repository.tabs().filter(t => t.lifecycle !== 'closed').length >= 1000) return failure('VALIDATION', 'The managed tab limit has been reached');
      const ordinal = tabs.reduce((max, t) => Math.max(max, t.ordinal), -1) + 1;
      const tab = this.intent(session, input.title ?? `Shell ${ordinal + 1}`, ordinal);
      this.repository.transaction(() => {
        this.repository.saveTab(tab); this.saveIntent(tab, 'add');
        const current = this.repository.session(session.id)!;
        current.updatedAt = new Date().toISOString(); this.repository.saveSession(current);
      });
      this.changed(); await this.dispatch(session.id, tab.id, expectedTarget);
      return success(this.toDto(this.repository.session(session.id)!));
    });
  }
  private stickyControl(id: string, error: AppError): void {
    const session = this.repository.session(id);
    if (session) { session.error = error; session.updatedAt = new Date().toISOString(); this.repository.saveSession(session); this.changed(); }
  }
  private clearRecoveredFocusError(id: string): void {
    const current = this.repository.session(id);
    if (current?.error?.code === 'FOCUS_DENIED') { current.error = null; this.repository.saveSession(current); this.changed(); }
  }
  focusSession(input: { sessionId: string }): Promise<Result<{ focused: true }>> {
    if (!requestSchemas.focusSession.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid session ID'));
    return this.serialize(input.sessionId, async () => {
      const session = this.repository.session(input.sessionId);
      if (!session) return failure('NOT_FOUND', 'Session not found');
      if (!this.probe.capabilities.focusWindow) return failure('UNSUPPORTED', 'Native focus is unavailable');
      if (this.windows) {
        if (!session.binding) return failure('TARGET_LOST','No authenticated window generation is available. No replacement was launched.',true);
        const result = await this.windows.focus(session.id);
        if (!result.ok) this.stickyControl(session.id,result.error);
        else this.clearRecoveredFocusError(session.id);
        return result;
      }
      const verified = await this.verify(session);
      if (!verified.ok) { this.stickyControl(session.id, verified.error); return verified; }
      const response = await this.native(() => this.backend.focus(verified.value), z.object({ focused: z.literal(true) }).strict());
      if (!response.ok) this.stickyControl(session.id, response.error);
      else {
        const current = this.repository.session(session.id)!;
        if (current.error?.code === 'FOCUS_DENIED' || current.error?.code === 'TARGET_LOST' || current.error?.code === 'TARGET_AMBIGUOUS') { current.error = null; this.repository.saveSession(current); this.changed(); }
      }
      return response;
    });
  }
  activateSession(input: { sessionId: string }): Promise<Result<SessionDto>> {
    if (!requestSchemas.activateSession.safeParse(input).success) return Promise.resolve(failure('VALIDATION','Invalid session ID'));
    const pending = this.activations.get(input.sessionId);
    if (pending) return pending;
    const action = this.serialize(input.sessionId,async () => {
      if (!this.windows) return failure('UNSUPPORTED','This backend cannot confirm window closure or reopen a saved session');
      const result = await this.windows.activate(input.sessionId);
      if (!result.ok) { if (result.error.code === 'FOCUS_DENIED') this.stickyControl(input.sessionId,result.error); return result; }
      if (result.value.windowState === 'alive') this.clearRecoveredFocusError(input.sessionId);
      return success(this.toDto(this.repository.session(input.sessionId)!));
    });
    this.activations.set(input.sessionId,action);
    void action.finally(()=>{if(this.activations.get(input.sessionId)===action)this.activations.delete(input.sessionId);});
    return action;
  }
  refreshSessionMembership(input: { sessionId: string }): Promise<Result<SessionDto>> {
    if (!requestSchemas.refreshSessionMembership.safeParse(input).success) return Promise.resolve(failure('VALIDATION','Invalid session ID'));
    return this.serialize(input.sessionId,async()=>{
      if (!this.windows) return failure('UNSUPPORTED','Explicit membership is unavailable');
      const result=await this.windows.reconcile(input.sessionId);
      return result.ok ? success(this.toDto(result.value)) : result;
    });
  }
  prepareSessionRegistration(input: { sessionId: string; shellId: SettingsDto['shellId'] }): Promise<Result<import('../shared/contracts').RegistrationGuideDto>> {
    if (!requestSchemas.prepareSessionRegistration.safeParse(input).success) return Promise.resolve(failure('VALIDATION','Invalid registration request'));
    return this.serialize(input.sessionId,async()=>this.windows ? this.windows.prepare(input.sessionId,input.shellId) : failure('UNSUPPORTED','Explicit terminal registration is unavailable'));
  }
  setSessionEnv(input: { sessionId: string; env: EnvVar[] }): Promise<Result<SessionDto>> {
    return Promise.resolve(failure('UNSUPPORTED', 'Environment overrides require embedded terminals.'));
  }
  setCliIntegration(_input: { installed: boolean }) { return Promise.resolve(failure('UNSUPPORTED', 'Shellfox CLI integration requires embedded terminals.')); }
  renameSession(input: { sessionId: string; title: string }): Promise<Result<SessionDto>> {
    if (!requestSchemas.renameSession.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid title'));
    return this.mutate(input.sessionId, s => { s.title = input.title; });
  }
  settleSession(input: { sessionId: string; confirmActive: boolean }): Promise<Result<SessionDto>> {
    if (!requestSchemas.settleSession.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid settle request'));
    return this.mutate(input.sessionId, s => {
      const activity = this.repository.tabs(s.id).map(tab => {
        const observation = this.observations.get(tab.id);
        return { lifecycle: tab.lifecycle, status: tabStatus(tab.lifecycle, null, observation), agents: observation?.root === 'alive' ? observation.agents : 0 };
      });
      if (!input.confirmActive && needsSettleConfirmation(activity)) return failure('SETTLE_CONFIRM_REQUIRED', 'Running or unknown terminals remain active. Confirm settling without stopping them.');
      s.settledAt = s.settledAt ?? new Date().toISOString();
    }, 'history');
  }
  unsettleSession(input: { sessionId: string }): Promise<Result<SessionDto>> {
    if (!requestSchemas.unsettleSession.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid session ID'));
    return this.mutate(input.sessionId, s => { s.settledAt = null; }, 'history');
  }
  clearSessionError(input: { sessionId: string }): Promise<Result<SessionDto>> {
    if (!requestSchemas.clearSessionError.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid session ID'));
    return this.mutate(input.sessionId, s => { s.error = null; for (const tab of this.repository.tabs(s.id)) { tab.error = null; this.repository.saveTab(tab); } });
  }
  private mutate(id: string, action: (session: SessionRecord) => Result<never> | void, reason: ChangedEvent['reason'] = 'sessions'): Promise<Result<SessionDto>> {
    return this.serialize(id, async () => {
      const session = this.repository.session(id);
      if (!session) return failure('NOT_FOUND', 'Session not found');
      let failed: Result<never> | void;
      this.repository.transaction(() => { failed = action(session); if (!failed) { session.updatedAt = new Date().toISOString(); this.repository.saveSession(session); } });
      if (failed!) return failed!;
      this.changed(reason); return success(this.toDto(session));
    });
  }
  retryTab(input: { tabId: string; confirmPossibleDuplicate: boolean }): Promise<Result<SessionDto>> {
    if (!requestSchemas.retryTab.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid retry request'));
    const initial = this.repository.tab(input.tabId);
    if (!initial) return Promise.resolve(failure('NOT_FOUND', 'Tab not found'));
    return this.serialize(initial.sessionId, async () => {
      const tab = this.repository.tab(input.tabId)!;
      const session = this.repository.session(tab.sessionId)!;
      if (session.settledAt || tab.lifecycle === 'open' || tab.lifecycle === 'launching') return failure('UNSUPPORTED', 'Only an unsettled failed or uncertain launch can be retried');
      if (tab.lifecycle === 'launch-uncertain' && !input.confirmPossibleDuplicate) return failure('RETRY_CONFIRM_REQUIRED', 'Another native window may already exist. Confirm before creating another terminal.');
      if (!this.probe.available || !this.probe.capabilities.createWindow) return failure('UNSUPPORTED', 'Native launches are unavailable');
      if (session.target) { const verified = await this.verify(session); if (!verified.ok) { this.stickyControl(session.id, verified.error); return verified; } }
      try { await this.directoryValidator(tab.cwd); } catch { return failure('VALIDATION', 'The default directory is no longer accessible'); }
      this.repository.transaction(() => {
        tab.operationId = randomUUID(); tab.lifecycle = 'launching'; tab.registration = null;
        this.observations.delete(tab.id); this.repository.saveTab(tab); this.saveIntent(tab, 'retry');
        const current = this.repository.session(session.id)!;
        current.updatedAt = new Date().toISOString(); this.repository.saveSession(current);
      });
      this.changed(); await this.dispatch(session.id, tab.id);
      return success(this.toDto(this.repository.session(session.id)!));
    });
  }
  saveSettings(input: SettingsDto): Promise<Result<SettingsDto>> {
    const parsed = requestSchemas.saveSettings.safeParse(input);
    if (!parsed.success) return Promise.resolve(failure('VALIDATION', 'Invalid settings'));
    return this.serialize('settings', async () => {
      const previous = this.repository.settings();
      // Legacy adapters select by shellId, not embedded terminalProfileId.
      const selected = this.probe.shells.find(s => s.available && s.id === parsed.data.shellId);
      const sameExecutable = (a: string, b: string | null): boolean => b !== null && path.normalize(a).toLowerCase() === path.normalize(b).toLowerCase();
      if (parsed.data.shellExecutable) {
        const unchangedExecutable = sameExecutable(parsed.data.shellExecutable, previous.shellExecutable);
        if (!selected || !sameExecutable(parsed.data.shellExecutable, selected.executable) && !unchangedExecutable) return failure('VALIDATION', 'Choose a discovered supported shell executable');
      }
      const settings: SettingsDto = selected ? { ...parsed.data, shellId: selected.id, shellExecutable: selected.executable } : parsed.data;
      if (JSON.stringify(previous.processRules) === JSON.stringify(settings.processRules) && (this.watchReady || !this.probe.capabilities.processTracking)) {
        this.repository.saveSettings(settings); this.changed('settings'); return success(settings);
      }
      // Serialize native acceptance, persistence and rollback with every other watch update.
      return this.queueWatch(async () => {
        const applied = await this.replaceWatch(settings.processRules);
        if (!applied.ok) {
          await this.replaceWatch(previous.processRules);
          return applied;
        }
        try { this.repository.transaction(() => this.repository.saveSettings(settings)); }
        catch {
          await this.replaceWatch(previous.processRules);
          return failure('STORAGE_FAILED', 'Settings could not be saved. The previous rules were restored when available.', true);
        }
        this.changed('settings');
        return success(settings);
      });
    });
  }
  setExplorerIntegration(input: { installed: boolean }): Promise<Result<ExplorerIntegrationDto>> {
    if (!requestSchemas.setExplorerIntegration.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid integration request'));
    return this.serialize('explorer', async () => {
      if (!this.packagedExecutable) return failure('UNSUPPORTED', 'Explorer integration requires an installed packaged application');
      const response = await this.native(() => this.backend.setExplorerIntegration({ installed: input.installed, executablePath: this.packagedExecutable! }), explorerSchema);
      if (response.ok) { this.explorer = response.value; this.repository.saveExplorerPreference(input.installed); this.changed('settings'); }
      return response;
    });
  }
}
