import path from 'node:path';
import { randomUUID } from 'node:crypto';
import type { AppError, ChangedEvent, EnvVar, ExplorerIntegrationDto, HistoryPage, HistoryQuery, ManagerSnapshot, NativeProbe, RegistrationGuideDto, Result, SessionDto, SettingsDto, ShellId, TabDto, TerminalAttachmentDto, TerminalEvent, TerminalProfileDto, TerminalProfilesDto } from '../../shared/contracts';
import { failure, success } from '../../shared/contracts';
import { requestSchemas } from '../../shared/schemas';
import { aggregateStatus, countTabs, needsSettleConfirmation, publicStatus, sortSessions, tabStatus } from '../../shared/status';
import type { NativeInit, TabObservation } from '../../shared/native-port';
import type { OperationRecord, RepositoryPort, SessionRecord, TabRecord } from '../models';
import { unavailableExplorer, unavailableProbe, unavailableCli, defaultSettings } from '../defaults';
import { resolvePython } from './python';
import { PtyBackend } from './backend';
import { ProcessTracker, getProcessTrackingCapability } from './tracking';
import { closeGuest } from './guest-close';
import type { ExplorerPort } from '../platform/explorer';
import type { CliPort } from '../platform/shellfox-cli';

export const LIFETIME_NOTICE = 'Embedded shells run only while Shellfox is running. Quitting closes owned terminals after confirmation; restarting does not restore commands.';
const shellId = (p: TerminalProfileDto): ShellId => p.environment === 'wsl' ? 'wsl' : p.id === 'pwsh' ? 'pwsh' : p.id === 'windows-powershell' ? 'windows-powershell' : 'login-shell';
export interface TrackerPort { setWatch: ProcessTracker['setWatch']; dispose(): void; poll?: ProcessTracker['poll']; resolveIdentity?: ProcessTracker['resolveIdentity']; resolveDescendants?: ProcessTracker['resolveDescendants'] }
export class EmbeddedSessionService {
  revision = 0;
  probe: NativeProbe = { ...unavailableProbe('Embedded backend has not initialized.'), adapterId: 'embedded-pty' };
  cli = { ...unavailableCli };
  explorer: ExplorerIntegrationDto = { ...unavailableExplorer, reason: 'Explorer integration has not initialized.' };
  private readonly listeners = new Set<(event: ChangedEvent) => void>();
  private readonly streams = new Set<(event: TerminalEvent) => void>();
  private readonly queues = new Map<string, Promise<unknown>>();
  private readonly observations = new Map<string, TabObservation & { generation: string }>();
  private readonly tracker: TrackerPort;
  private readonly pendingExits = new Map<string, Extract<TerminalEvent, { type: 'exit' }>>();
  private exitRetryTimer?: ReturnType<typeof setInterval>;
  private readonly reportedExitFailures = new Set<string>();
  private unsubscribe?: () => void;
  private disposed = false;
  constructor(readonly repository: RepositoryPort, readonly backend = new PtyBackend(), trackerFactory: (emit: (items: TabObservation[]) => void) => TrackerPort = emit => new ProcessTracker(emit), private readonly explorerIntegration?: ExplorerPort, private readonly cliIntegration?: CliPort) {
    this.tracker = trackerFactory(items => {
      if (this.disposed) return;
      this.observations.clear();
      for (const owned of backend.live()) backend.rememberOwnership(owned.tabId, owned.generation, this.tracker.resolveIdentity?.(owned.root) ?? null, this.tracker.resolveDescendants?.(owned.root) ?? []);
      for (const item of items) {
        const tab = repository.tab(item.tabId), owned = backend.get(item.tabId);
        if (tab?.terminal && owned?.state === 'open' && owned.generation === tab.operationId && tab.sessionId === item.sessionId) this.observations.set(tab.id, { ...item, generation: owned.generation });
      }
      this.changed('native');
    });
    backend.configureGuestTermination(async (root, captured, known) => {
      await this.tracker.poll?.();
      await closeGuest(root, this.tracker.resolveIdentity?.(root) ?? captured, undefined, [...known, ...(this.tracker.resolveDescendants?.(root) ?? [])]);
    });
  }
  subscribe(listener: (event: ChangedEvent) => void): () => void { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; }
  subscribeTerminal(listener: (event: TerminalEvent) => void): () => void { this.streams.add(listener); return () => { this.streams.delete(listener); }; }
  selectSession(sessionId: string): void { this.changed('sessions', sessionId); }
  private changed(reason: ChangedEvent['reason'] = 'sessions', selectSessionId?: string): void {
    const event: ChangedEvent = { revision: ++this.revision, reason, ...(selectSessionId ? { selectSessionId } : {}) };
    for (const listener of this.listeners) { try { listener(event); } catch { /* Renderer failures do not roll back ownership. */ } }
  }
  private serialize<T>(id: string, action: () => Promise<Result<T>>): Promise<Result<T>> {
    const next = (this.queues.get(id) ?? Promise.resolve()).catch(() => undefined).then(() => this.disposed ? failure('NATIVE_UNAVAILABLE', 'Shellfox is shutting down.') : action()).catch(() => failure('STORAGE_FAILED', 'The embedded operation could not be saved.', true)) as Promise<Result<T>>;
    this.queues.set(id, next);
    void next.finally(() => { if (this.queues.get(id) === next) this.queues.delete(id); });
    return next;
  }
  async initialize(_input?: NativeInit): Promise<void> {
    this.unsubscribe = this.backend.subscribe(event => this.terminalEvent(event));
    this.backend.configurePythonPath(this.repository.settings().pythonPath ?? null);
    const result = await this.backend.initialize();
    if (this.explorerIntegration) {
      // Reapply saved opt-in with the new identity; get() also removes owned legacy verbs.
      const integration = this.repository.explorerPreference()
        ? await this.explorerIntegration.set(true) : await this.explorerIntegration.get();
      this.explorer = integration.ok ? integration.value : { ...unavailableExplorer, supported: ['win32', 'linux'].includes(process.platform), reason: integration.error.message };
    }
    if (this.cliIntegration) {
      // Round 3 already used Shellfox shims. Refresh their executable after the rename.
      const integration = this.repository.cliPreference?.() === true
        ? await this.cliIntegration.set(true) : await this.cliIntegration.get();
      this.cli = integration.ok ? integration.value : { ...unavailableCli, supported: ['win32', 'linux'].includes(process.platform), reason: integration.error.message };
    }
    const profiles = this.backend.getProfiles();
    const tracking = await getProcessTrackingCapability();
    const available = result.ok && profiles.profiles.some(p => p.available);
    this.probe = { ...this.probe, available, platform: process.platform, arch: process.arch,
      ...(process.platform === 'linux' ? { python: this.backend.getPython() } : {}),
      shells: profiles.profiles.filter(p => p.environment === 'local').map(p => ({ id: shellId(p), executable: p.executable, available: p.available, reason: null })),
      reasons: [LIFETIME_NOTICE, ...(result.ok ? [] : [result.error.message]), ...profiles.profiles.filter(p => !p.available).map(p => p.unavailableReason ?? `${p.label} is unavailable.`), ...(tracking.reason ? [tracking.reason] : [])],
      capabilities: { createWindow: available, addTab: available, focusWindow: available, activateTab: available, closeTerminal: available, splitPane: false, attachExisting: false, commandExitStatus: false, processTracking: tracking.available, explorerContextMenu: this.explorer.supported, embeddedTerminal: true, terminalLifetime: 'app-owned', shellSurvival: false } };
    this.repository.transaction(() => {
      // An app-owned handle cannot be reconstituted from its old PID. External roots are untouched.
      for (const tab of this.repository.tabs()) if (tab.terminal && tab.lifecycle !== 'closed') {
        tab.lifecycle = 'closed'; tab.terminal.exitCode = null;
        this.repository.saveTab(tab);
        const op = this.repository.operation(tab.operationId);
        if (op && op.state !== 'registered') this.repository.saveOperation({ ...op, state: 'failed', updatedAt: new Date().toISOString(), error: { code: 'NATIVE_UNAVAILABLE', message: 'The previous app-owned shell is no longer attached. Open this session to start a fresh shell.', retryable: true } });
      }
      // Retired external windows are history, never app-owned runtime sessions.
      // Idempotent metadata-only migration: do not observe, focus, launch or kill their processes.
      for (const session of this.repository.sessions()) {
        if (!session.settledAt && (session.adapterId !== 'embedded-pty' || !this.repository.tabs(session.id).some(tab => tab.terminal))) {
          session.settledAt = session.updatedAt = new Date().toISOString();
          this.repository.saveSession(session);
        }
      }
      const settings = this.repository.settings();
      if (settings.adapterId !== 'embedded-pty') {
        const selected = profiles.profiles.find(p => p.id === settings.shellId && p.available) ?? profiles.profiles.find(p => p.id === profiles.defaultProfileId && p.available);
        // Expand only recognizable bundled executable names for host/WSL matching. Custom rules are unchanged.
        const rules = settings.processRules.map(rule => {
          const bundled = defaultSettings.processRules.find(r => r.id === rule.id && r.label === rule.label);
          if (!bundled || rule.executablePaths.length || JSON.stringify(rule.executableBasenames) !== JSON.stringify(bundled.executableBasenames)) return rule;
          return { ...rule, executableBasenames: [...new Set(rule.executableBasenames.flatMap(n => [n, n.replace(/\.exe$/, '')]))] };
        });
        this.repository.saveSettings({ ...settings, adapterId: 'embedded-pty', shellId: selected ? shellId(selected) : settings.shellId, shellExecutable: selected?.executable ?? null, terminalProfileId: selected?.id ?? null, processRules: rules });
      }
    });
    this.refreshWatch(); this.changed('native');
  }
  private refreshWatch(): void {
    const live = this.backend.live();
    // Adding/closing another tab does not invalidate evidence for an unchanged
    // shell generation. Drop closed/replaced roots, not the entire session.
    for (const [id, observation] of this.observations) if (!live.some(e => e.tabId === id && e.state === 'open' && e.generation === observation.generation)) this.observations.delete(id);
    this.tracker.setWatch(live.map(e => e.root), this.repository.settings().processRules);
  }
  private terminalEvent(event: TerminalEvent): void {
    const owned = this.backend.get(event.tabId);
    if (!owned || event.type !== 'activity' && owned.generation !== event.generation) return;
    // Runtime identity, not SQLite availability, authorizes this stream. Forward before persistence.
    this.forwardTerminal(event);
    if (event.type !== 'exit') return;
    this.backend.rememberOwnership(owned.tabId, owned.generation, this.tracker.resolveIdentity?.(owned.root) ?? null, this.tracker.resolveDescendants?.(owned.root) ?? []);
    this.pendingExits.set(event.tabId, event);
    this.observations.delete(event.tabId);
    this.reconcileExits();
    try { this.refreshWatch(); } catch { /* Exit evidence survives a broken watch/storage dependency. */ }
    this.changed('native');
  }
  private forwardTerminal(event: TerminalEvent): void { for (const listener of this.streams) { try { listener(event); } catch { /* Never suppress other viewers. */ } } }
  private reconcileExits(): void {
    for (const [id, exit] of this.pendingExits) {
      try {
        const tab = this.repository.tab(id);
        if (tab?.terminal && tab.operationId === exit.generation) { tab.lifecycle = 'closed'; tab.terminal.exitCode = exit.exitCode; this.repository.saveTab(tab); }
        this.pendingExits.delete(id); this.reportedExitFailures.delete(id);
      } catch {
        if (this.reportedExitFailures.has(id)) continue;
        this.reportedExitFailures.add(id);
        this.forwardTerminal({ type: 'error', tabId: exit.tabId, generation: exit.generation, error: { code: 'STORAGE_FAILED', message: 'Shell exit is confirmed, but saved metadata could not be updated. Runtime closure remains authoritative and metadata will be retried.', retryable: true } });
      }
    }
    if (this.pendingExits.size && !this.exitRetryTimer) { this.exitRetryTimer = setInterval(() => { this.reconcileExits(); this.changed('native'); }, 1000); this.exitRetryTimer.unref?.(); }
    else if (!this.pendingExits.size && this.exitRetryTimer) { clearInterval(this.exitRetryTimer); this.exitRetryTimer = undefined; }
  }
  toDto(session: SessionRecord): SessionDto {
    const tabs: TabDto[] = this.repository.tabs(session.id).map(tab => {
      const owned = this.backend.get(tab.id), embedded = !!tab.terminal;
      const latestObservation = this.observations.get(tab.id);
      const observation = latestObservation?.generation === tab.operationId ? latestObservation : undefined;
      const exit = this.pendingExits.get(tab.id);
      const runtimeClosed = owned?.generation === tab.operationId && owned.state === 'closed' || exit?.generation === tab.operationId;
      const lifecycle = runtimeClosed ? 'closed' : tab.lifecycle;
      return { id: tab.id, sessionId: tab.sessionId, title: tab.title, cwd: tab.cwd, ordinal: tab.ordinal, createdAt: tab.createdAt,
        lifecycle, status: tabStatus(lifecycle, tab.error, embedded ? observation : undefined),
        agents: embedded && owned?.state === 'open' && observation?.root === 'alive' ? observation.agents : 0,
        monitoringReason: !embedded ? 'Legacy external terminal metadata only. Its process is not adopted, watched or terminated.' : lifecycle === 'closed' ? owned?.cleanupPending ? 'The root shell exited, but owned descendant cleanup is still pending. No replacement or complete shutdown is claimed.' : exit ? 'Runtime shell exit is confirmed. Saved closure metadata is being retried.' : 'The app-owned shell is closed. Commands are not restored.' : observation ? observation.reason ?? (observation.agents > 0 ? 'A matched agent process is running in this shell.' : 'The shell is alive; no matched agent process is running.') : 'Awaiting owned shell process evidence.',
        error: tab.error, generation: tab.operationId, terminalKind: embedded ? 'embedded' : 'external-legacy', profileId: tab.terminal?.profileId ?? null, exitCode: exit?.generation === tab.operationId ? exit.exitCode : owned?.generation === tab.operationId ? owned.exitCode : tab.terminal?.exitCode ?? null };
    });
    const embedded = session.adapterId === 'embedded-pty', live = this.backend.live().some(e => e.sessionId === session.id);
    const activity = aggregateStatus(tabs, session.error);
    return { id: session.id, title: session.title, cwd: session.cwd, adapterId: session.adapterId, shellId: session.shellId, createdAt: session.createdAt, updatedAt: session.updatedAt, settledAt: session.settledAt, pinnedAt: session.pinnedAt ?? null,
      status: publicStatus(activity, session.settledAt), activityStatus: activity, counts: countTabs(tabs), tabs, env: session.env ?? [], canFocus: live,
      canAddTab: this.probe.available && !session.settledAt, controlReason: embedded ? LIFETIME_NOTICE : 'Legacy external terminals are not controlled. Activating this task starts a separate embedded shell.', error: session.error,
      terminalLifetime: embedded ? 'app-owned' : 'external-legacy', shellSurvival: false,
      window: { state: 'unsupported', canReopen: false, canRegister: false, reason: 'Embedded tabs do not use native window ownership or external shell registration.' } };
  }
  getSnapshot(): Result<ManagerSnapshot> { return success({ revision: this.revision, sessions: sortSessions(this.repository.sessions().filter(s => !s.settledAt).map(s => this.toDto(s))), settings: this.repository.settings(), probe: this.probe, explorer: this.explorer, cli: this.cli }); }
  getHistory(input: HistoryQuery): Result<HistoryPage> {
    const parsed = requestSchemas.getHistory.safeParse(input); if (!parsed.success) return failure('VALIDATION', 'Invalid history query.');
    const { search, status, page, pageSize } = parsed.data;
    const items = this.repository.settled(search).map(s => this.toDto(s)).filter(s => status === 'all' || status === 'settled' || s.activityStatus === status);
    return success({ items: items.slice((page - 1) * pageSize, page * pageSize), total: items.length, page, pageSize });
  }
  getTerminalProfiles(): Result<TerminalProfilesDto> { return success(this.backend.getProfiles()); }
  attachTerminal(input: Parameters<PtyBackend['attach']>[0]): Result<TerminalAttachmentDto> {
    const tab = this.repository.tab(input.tabId);
    if (!tab?.terminal) return failure('UNSUPPORTED', 'Only owned embedded terminals can be attached.');
    return this.backend.attach(input);
  }
  writeTerminal(input: Parameters<PtyBackend['write']>[0]): Result<{ written: true }> { return this.backend.write(input); }
  resizeTerminal(input: Parameters<PtyBackend['resize']>[0]): Result<{ resized: true }> { return this.backend.resize(input); }
  private profile(session?: SessionRecord, explicit?: string): TerminalProfileDto | undefined {
    const profiles = this.backend.getProfiles(), settings = this.repository.settings();
    const existing = session && this.repository.tabs(session.id).find(t => t.terminal)?.terminal?.profileId;
    const id = explicit ?? existing ?? settings.terminalProfileId ?? profiles.defaultProfileId;
    return profiles.profiles.find(p => p.id === id && p.available);
  }
  createSession(input: { cwd: string; requestId: string; title?: string }): Promise<Result<SessionDto>> {
    if (!requestSchemas.createSession.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid session request.'));
    return this.serialize('create', async () => {
      const existing = this.repository.sessionByRequest(input.requestId); if (existing) return success(this.toDto(existing));
      const profile = this.profile(); if (!profile || !this.probe.available) return failure('DEPENDENCY_MISSING', 'No discovered interactive terminal profile is available.');
      if (this.repository.sessions().filter(s => !s.settledAt).length >= 10000) return failure('VALIDATION', 'The saved session limit is reached.');
      const now = new Date().toISOString();
      const session: SessionRecord = { id: randomUUID(), title: input.title ?? (path.basename(input.cwd).slice(0, 200) || input.cwd.slice(0, 200)), cwd: input.cwd, adapterId: 'embedded-pty', shellId: shellId(profile), shellExecutable: profile.executable, createdAt: now, updatedAt: now, settledAt: null, error: null, target: null, binding: null, windowState: 'unknown' };
      return this.open(session, profile, 'create', input.requestId);
    });
  }
  activateSession(input: { sessionId: string }): Promise<Result<SessionDto>> {
    if (!requestSchemas.activateSession.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid session ID.'));
    return this.serialize(input.sessionId, async () => {
      const session = this.repository.session(input.sessionId); if (!session) return failure('NOT_FOUND', 'Session not found.');
      if (this.backend.live().some(e => e.sessionId === session.id && e.state === 'open') || this.repository.tabs(session.id).some(t => t.terminal && t.lifecycle === 'launching')) return success(this.toDto(session));
      const cleanup = await this.backend.awaitCleanup(session.id); if (!cleanup.ok) return cleanup;
      const profile = this.profile(session); if (!profile || !this.probe.available) return failure('DEPENDENCY_MISSING', 'The session terminal profile is unavailable.');
      return this.open(session, profile, 'reopen');
    });
  }
  addTab(input: { sessionId: string; title?: string; profileId?: string; cwd?: string }): Promise<Result<SessionDto>> {
    if (!requestSchemas.addTab.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid new tab request.'));
    return this.serialize(input.sessionId, async () => {
      const session = this.repository.session(input.sessionId); if (!session) return failure('NOT_FOUND', 'Session not found.');
      if (session.settledAt) return failure('UNSUPPORTED', 'Unsettle this session before adding a tab.');
      const profile = this.profile(session, input.profileId); if (!profile || !this.probe.available) return failure('DEPENDENCY_MISSING', 'The selected terminal profile is unavailable.');
      return this.open(session, profile, 'add', null, input.title, input.cwd);
    });
  }
  private async open(session: SessionRecord, profile: TerminalProfileDto, kind: OperationRecord['kind'], requestId: string | null = null, title?: string, cwd?: string): Promise<Result<SessionDto>> {
    const tabs = this.repository.tabs(session.id);
    if (tabs.length >= 1000 || this.backend.live().length >= 64) return failure('VALIDATION', 'The embedded terminal limit is reached.');
    const ordinal = tabs.reduce((n, t) => Math.max(n, t.ordinal), -1) + 1, now = new Date().toISOString();
    const tab: TabRecord = { id: randomUUID(), sessionId: session.id, title: title ?? `Shell ${ordinal + 1}`, cwd: cwd ?? session.cwd, ordinal, createdAt: now, lifecycle: 'launching', operationId: randomUUID(), registration: null, error: null, terminal: { kind: 'embedded', profileId: profile.id, exitCode: null } };
    const operation: OperationRecord = { id: tab.operationId, sessionId: session.id, tabId: tab.id, requestId, kind, state: 'intent', createdAt: now, updatedAt: now, error: null };
    this.repository.transaction(() => { this.repository.saveSession(session); this.repository.saveTab(tab); this.repository.saveOperation(operation); }); this.changed();
    const launched = await this.backend.launch({ tabId: tab.id, sessionId: session.id, generation: tab.operationId, profileId: profile.id, cwd: tab.cwd, env: this.repository.session(session.id)?.env ?? session.env ?? [], ...(this.cli.installed && this.cliIntegration ? { cliBin: this.cliIntegration.binDir } : {}) });
    try {
      this.repository.transaction(() => {
        const current = this.repository.tab(tab.id)!;
        const latest = this.repository.session(session.id)!;
        if (!launched.ok) { const uncertain = this.backend.get(tab.id)?.state === 'open'; current.lifecycle = uncertain ? 'launch-uncertain' : 'closed'; current.error = launched.error; operation.state = uncertain ? 'uncertain' : 'failed'; operation.error = launched.error; }
        else {
          current.cwd = launched.value.cwd; current.lifecycle = launched.value.state === 'open' ? 'open' : 'closed'; current.terminal!.exitCode = launched.value.exitCode; operation.state = 'registered';
          latest.adapterId = 'embedded-pty'; latest.shellId = shellId(profile); latest.shellExecutable = profile.executable; latest.target = null; latest.binding = null;
        }
        latest.updatedAt = new Date().toISOString(); operation.updatedAt = latest.updatedAt;
        this.repository.saveTab(current); this.repository.saveOperation(operation); this.repository.saveSession(latest);
      });
    } catch {
      if (launched.ok) await this.backend.close({ tabId: tab.id, generation: tab.operationId });
      throw new Error('Ownership persistence failed.');
    }
    this.refreshWatch(); this.changed();
    return success(this.toDto(this.repository.session(session.id)!));
  }
  closeTab(input: { tabId: string; generation: string }): Promise<Result<SessionDto>> {
    if (!requestSchemas.closeTab.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid close request.'));
    const initial = this.repository.tab(input.tabId); if (!initial) return Promise.resolve(failure('NOT_FOUND', 'Tab not found.'));
    return this.serialize(initial.sessionId, async () => {
      const tab = this.repository.tab(input.tabId)!;
      if (!tab.terminal) return failure('UNSUPPORTED', 'Legacy external processes are not owned by this manager.');
      if (tab.operationId !== input.generation) return failure('TARGET_LOST', 'This terminal generation is stale.');
      if (tab.lifecycle === 'closed' && !this.backend.get(tab.id)) return success(this.toDto(this.repository.session(tab.sessionId)!));
      const closed = await this.backend.close(input); if (!closed.ok) return closed;
      // The PTY exit event is authoritative. A pending OS close must not authorize a replacement shell.
      this.changed(); return success(this.toDto(this.repository.session(tab.sessionId)!));
    });
  }
  async focusSession(input: { sessionId: string }): Promise<Result<{ focused: true }>> {
    if (!requestSchemas.focusSession.safeParse(input).success) return failure('VALIDATION', 'Invalid session ID.');
    return this.backend.live().some(e => e.sessionId === input.sessionId) ? success({ focused: true }) : failure('TARGET_LOST', 'No live embedded shell. Activate the session to open one.');
  }
  refreshSessionMembership(input: { sessionId: string }): Promise<Result<SessionDto>> { const s = this.repository.session(input.sessionId); return Promise.resolve(s ? success(this.toDto(s)) : failure('NOT_FOUND', 'Session not found.')); }
  prepareSessionRegistration(_input: { sessionId: string; shellId: ShellId }): Promise<Result<RegistrationGuideDto>> { return Promise.resolve(failure('UNSUPPORTED', 'External process adoption is disabled for embedded sessions.')); }
  setSessionEnv(input: { sessionId: string; env: EnvVar[] }): Promise<Result<SessionDto>> {
    const parsed = requestSchemas.setSessionEnv.safeParse(input);
    if (!parsed.success) return Promise.resolve(failure('VALIDATION', 'Invalid environment variables.'));
    return this.mutate(parsed.data.sessionId, session => { session.env = parsed.data.env; });
  }
  setSessionPinned(input: { sessionId: string; pinned: boolean }): Promise<Result<SessionDto>> {
    if (!requestSchemas.setSessionPinned.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid pin request.'));
    return this.mutate(input.sessionId, s => {
      if (s.settledAt) return failure('UNSUPPORTED', 'Restore this session before pinning it.');
      s.pinnedAt = input.pinned ? s.pinnedAt ?? new Date().toISOString() : null;
    });
  }
  renameSession(input: { sessionId: string; title: string }): Promise<Result<SessionDto>> {
    if (!requestSchemas.renameSession.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid title.'));
    return this.mutate(input.sessionId, s => { s.title = input.title; });
  }
  settleSession(input: { sessionId: string; confirmActive: boolean }): Promise<Result<SessionDto>> {
    if (!requestSchemas.settleSession.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid settle request.'));
    return this.mutate(input.sessionId, s => {
      const tabs = this.toDto(s).tabs.map(t => ({ ...t, status: tabStatus(t.lifecycle, null, this.observations.get(t.id)) }));
      if (!input.confirmActive && needsSettleConfirmation(tabs)) return failure('SETTLE_CONFIRM_REQUIRED', 'Running or unknown shells may remain active. Settling does not stop them.');
      s.settledAt ??= new Date().toISOString();
    }, 'history');
  }
  unsettleSession(input: { sessionId: string }): Promise<Result<SessionDto>> { return this.mutate(input.sessionId, s => {
    if (s.adapterId !== 'embedded-pty' || !this.repository.tabs(s.id).some(tab => tab.terminal)) return failure('UNSUPPORTED', 'Legacy external sessions are read-only history. Create a new embedded session instead.');
    s.settledAt = null;
  }, 'history'); }
  clearSessionError(input: { sessionId: string }): Promise<Result<SessionDto>> { return this.mutate(input.sessionId, s => { s.error = null; for (const tab of this.repository.tabs(s.id)) { tab.error = null; this.repository.saveTab(tab); } }); }
  private mutate(id: string, action: (s: SessionRecord) => void | Result<never>, reason: ChangedEvent['reason'] = 'sessions'): Promise<Result<SessionDto>> {
    if (!requestSchemas.activateSession.safeParse({ sessionId: id }).success) return Promise.resolve(failure('VALIDATION', 'Invalid session ID.'));
    return this.serialize(id, async () => {
      const s = this.repository.session(id); if (!s) return failure('NOT_FOUND', 'Session not found.');
      const result = action(s); if (result) return result;
      s.updatedAt = new Date().toISOString(); this.repository.saveSession(s); this.changed(reason); return success(this.toDto(s));
    });
  }
  retryTab(input: { tabId: string; confirmPossibleDuplicate: boolean }): Promise<Result<SessionDto>> {
    if (!requestSchemas.retryTab.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid retry.'));
    const tab = this.repository.tab(input.tabId);
    if (!tab?.terminal) return Promise.resolve(failure('UNSUPPORTED', 'Legacy external launch retries are disabled.'));
    return this.activateSession({ sessionId: tab.sessionId });
  }
  saveSettings(input: SettingsDto): Promise<Result<SettingsDto>> {
    const parsed = requestSchemas.saveSettings.safeParse(input); if (!parsed.success) return Promise.resolve(failure('VALIDATION', 'Invalid settings.'));
    return this.serialize('settings', async () => {
      const previous = this.repository.settings();
      const pythonChanged = process.platform === 'linux' && (parsed.data.pythonPath ?? null) !== (previous.pythonPath ?? null);
      if (pythonChanged) {
        const check = await resolvePython(parsed.data.pythonPath, {}, parsed.data.pythonPath !== null);
        if (!check.usable) return failure('VALIDATION', "This Python can't be used: " + check.reason);
        await this.backend.refreshProfiles(parsed.data.pythonPath);
      }
      const profiles = this.backend.getProfiles(), requested = input.terminalProfileId ?? profiles.profiles.find(p => p.id === input.shellId && (!input.shellExecutable || p.executable === input.shellExecutable))?.id ?? profiles.defaultProfileId;
      const selected = profiles.profiles.find(p => p.id === requested && p.available);
      if (!selected) { if (pythonChanged) await this.backend.refreshProfiles(previous.pythonPath ?? null); return failure('VALIDATION', 'Choose a discovered terminal profile.'); }
      // An explicit discovered profile owns its derived fields. Full DTOs may still carry the old executable.
      const unchangedExecutable = !!input.terminalProfileId && input.shellExecutable === previous.shellExecutable;
      if (input.shellExecutable && input.shellExecutable !== selected.executable && !unchangedExecutable) { if (pythonChanged) await this.backend.refreshProfiles(previous.pythonPath ?? null); return failure('VALIDATION', 'Arbitrary shell executables are not allowed.'); }
      const settings: SettingsDto = { ...parsed.data, adapterId: 'embedded-pty', terminalProfileId: selected.id, shellId: shellId(selected), shellExecutable: selected.executable };
      try { this.observations.clear(); this.tracker.setWatch(this.backend.live().map(e => e.root), settings.processRules); this.repository.saveSettings(settings); }
      catch { if (pythonChanged) await this.backend.refreshProfiles(previous.pythonPath ?? null); this.tracker.setWatch(this.backend.live().map(e => e.root), previous.processRules); return failure('STORAGE_FAILED', 'Settings could not be applied.', true); }
      if (pythonChanged) {
        const available = profiles.profiles.some(p => p.available);
        this.probe = { ...this.probe, available, python: this.backend.getPython(), reasons: [LIFETIME_NOTICE, ...profiles.profiles.filter(p => !p.available).map(p => p.unavailableReason ?? 'Shell unavailable')], shells: profiles.profiles.filter(p => p.environment === 'local').map(p => ({ id: shellId(p), executable: p.executable, available: p.available, reason: p.unavailableReason ?? null })), capabilities: { ...this.probe.capabilities, createWindow: available, addTab: available, closeTerminal: available, focusWindow: available, activateTab: available } };
      }
      this.changed('settings'); return success(settings);
    });
  }
  private integrationRefresh?: Promise<void>;
  async refreshIntegrations(): Promise<void> {
    if (this.integrationRefresh) return this.integrationRefresh;
    this.integrationRefresh = (async () => {
      if (this.explorerIntegration) {
        const current = await this.explorerIntegration.get();
        this.explorer = current.ok ? current.value : { ...this.explorer, installed: false, reason: current.error.message };
      }
      if (this.cliIntegration) {
        const current = await this.cliIntegration.get();
        this.cli = current.ok ? current.value : { ...this.cli, installed: false, reason: current.error.message };
      }
    })().finally(() => { this.integrationRefresh = undefined; });
    return this.integrationRefresh;
  }
  setCliIntegration(input: { installed: boolean }) {
    if (!requestSchemas.setCliIntegration.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid Shellfox CLI request.'));
    return this.serialize('cli', async () => {
      if (!this.cliIntegration) return failure('UNSUPPORTED', 'Shellfox CLI integration requires Windows.');
      const result = await this.cliIntegration.set(input.installed);
      if (!result.ok) return result;
      this.cli = result.value; this.repository.saveCliPreference?.(input.installed);
      this.changed('settings'); return result;
    });
  }
  setExplorerIntegration(input: { installed: boolean }): Promise<Result<ExplorerIntegrationDto>> {
    if (!requestSchemas.setExplorerIntegration.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid integration request.'));
    return this.serialize('explorer', async () => {
      if (!this.explorerIntegration) return failure('UNSUPPORTED', 'Explorer integration requires Windows.');
      const result = await this.explorerIntegration.set(input.installed);
      if (!result.ok) return result;
      this.explorer = result.value;
      this.repository.saveExplorerPreference(input.installed);
      this.changed('settings');
      return result;
    });
  }
  ownedTerminalCount(): number { return this.backend.live().length + this.repository.tabs().filter(t => t.terminal && t.lifecycle === 'launching' && !this.backend.get(t.id)).length; }
  async dispose(): Promise<void> {
    if (this.disposed) return;
    this.disposed = true;
    await Promise.allSettled([...this.queues.values()]);
    try { await this.backend.dispose(); }
    catch { this.disposed = false; this.refreshWatch(); throw new Error('Owned terminal shutdown failed. The manager and failed handles remain available for retry.'); }
    this.reconcileExits();
    if (this.pendingExits.size) { this.disposed = false; throw new Error('Shells terminated, but exit metadata remains unsaved. Storage reconciliation is pending.'); }
    this.tracker.dispose();
    if (this.exitRetryTimer) clearInterval(this.exitRetryTimer);
    this.unsubscribe?.(); this.listeners.clear(); this.streams.clear();
  }
}
