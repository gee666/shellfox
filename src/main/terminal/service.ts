import path from 'node:path';
import { randomUUID } from 'node:crypto';
import type { SessionStatsDto, AppError, ChangedEvent, EnvVar, ExplorerIntegrationDto, ManagerSnapshot, NativeProbe, RegistrationGuideDto, Result, SessionDto, SettingsDto, ShellId, TabDto, TerminalSnapshotDto as TerminalAttachmentDto, TerminalBackendEvent as TerminalEvent, TerminalProfileDto, TerminalProfilesDto } from '../../shared/contracts';
import { failure, success } from '../../shared/contracts';
import { parseWslUnc } from '../../shared/wsl-path';
import { requestSchemas } from '../../shared/schemas';
import { aggregateStatus, countTabs, publicStatus, sortSessions, tabStatus } from '../../shared/status';
import type { NativeInit, TabObservation } from '../../shared/native-port';
import type { OperationRecord, RepositoryPort, SessionRecord, TabRecord } from '../models';
import { unavailableProbe, defaultSettings } from '../defaults';
import { DesktopIntegrations } from '../integrations';
import { resolvePython } from './python';
import { PtyBackend } from './backend';
import { TerminalControlBroker, type TerminalControlView } from './control';
import { ProcessTracker, getProcessTrackingCapability, createSystemSnapshotProvider, type TrackingHelperOptions } from './tracking';
import { closeGuest } from './guest-close';
import { createSessionStatsCollector } from '../control-stats';
import { VisibilityInterest } from './visibility-interest';
import type { ExplorerPort } from '../platform/explorer';
import type { CliPort } from '../platform/shellfox-cli';
import type { ControlRequest, ControlResponse } from '../control-types';
import { controlRequestSchema } from '../control-protocol';
import { SessionLifecycle, processActivityKey } from '../session-lifecycle';

export const LIFETIME_NOTICE = 'Terminals run while their owning Shellfox host runs. Disconnecting a client does not stop them; stopping the host ends them. Restarting does not restore commands.';
const shellId = (p: TerminalProfileDto): ShellId => p.environment === 'wsl' ? 'wsl' : p.id === 'pwsh' ? 'pwsh' : p.id === 'windows-powershell' || p.elevation === 'administrator' ? 'windows-powershell' : 'login-shell';
export interface TrackerPort { setVisibleSessions?: ProcessTracker['setVisibleSessions']; setWatch: ProcessTracker['setWatch']; dispose(): void; poll?: ProcessTracker['poll']; resolveIdentity?: ProcessTracker['resolveIdentity']; resolveDescendants?: ProcessTracker['resolveDescendants'] }
export class EmbeddedSessionService {
  revision = 0;
  private readonly terminalControl: TerminalControlBroker;
  createTerminalView(send: Parameters<TerminalControlBroker['createView']>[0], purge?: (tabId: string) => void): TerminalControlView { return this.terminalControl.createView(send, purge); }
  probe: NativeProbe = { ...unavailableProbe('Embedded backend has not initialized.'), adapterId: 'embedded-pty' };
  private readonly integrations: DesktopIntegrations;
  get cli() { return this.integrations.cli; }
  get explorer() { return this.integrations.explorer; }
  private readonly listeners = new Set<(event: ChangedEvent) => void>();
  private readonly streams = new Set<(event: TerminalEvent) => void>();
  private readonly queues = new Map<string, Promise<unknown>>();
  private readonly observations = new Map<string, TabObservation & { generation: string }>();
  private readonly tracker: TrackerPort;
  private readonly lifecycle: SessionLifecycle;
  private readonly processKeys = new Map<string, string>();
  private readonly visibility = new VisibilityInterest(ids => this.tracker.setVisibleSessions?.(ids));
  setVisibleSessions(input: { sessionIds: string[] }): Result<{ configured: true }> {
    const parsed = requestSchemas.setVisibleSessions.safeParse(input);
    if (!parsed.success) return failure('VALIDATION', 'Invalid visible sessions.');
    this.visibility.setLocal(parsed.data.sessionIds);
    return success({ configured: true });
  }
  setRemoteVisibleSessions(clientId: string, input: { sessionIds: string[] }): Result<{ configured: true }> {
    const parsed = requestSchemas.setVisibleSessions.safeParse(input);
    if (!parsed.success) return failure('VALIDATION', 'Invalid visible sessions.');
    if (!this.visibility.setRemote(clientId, parsed.data.sessionIds)) return failure('VALIDATION', 'Too many remote polling views.');
    return success({ configured: true });
  }
  clearRemoteVisibleSessions(clientId: string): void { this.visibility.removeRemote(clientId); }
  private readonly pendingExits = new Map<string, Extract<TerminalEvent, { type: 'exit' }>>();
  private exitRetryTimer?: ReturnType<typeof setInterval>;
  private readonly reportedExitFailures = new Set<string>();
  private unsubscribe?: () => void;
  private unsubscribeTitle?: () => void;
  private disposed = false;
  private updateClosing = false;
  constructor(readonly repository: RepositoryPort, readonly backend = new PtyBackend(), trackerFactory: (emit: (items: TabObservation[]) => void) => TrackerPort = emit => new ProcessTracker(emit), explorerIntegration?: ExplorerPort, private readonly cliIntegration?: CliPort, private readonly helperOptions: TrackingHelperOptions = {}) {
    this.terminalControl = new TerminalControlBroker({
      identity: id => {
        const tab = this.repository.tab(id), owned = this.backend.get(id);
        return tab?.terminal && owned?.generation === tab.operationId ? owned : undefined;
      },
      // Existing delivery may drain a confirmed exit after retention removes
      // its tab. That does not authorize a new attachment to deleted metadata.
      snapshot: input => this.backend.attach(input), write: input => this.writeTerminal(input), resize: input => this.backend.resize(input),
    });
    this.integrations = new DesktopIntegrations(explorerIntegration, cliIntegration, () => !this.disposed && !this.updateClosing, () => this.changed('settings'));
    let expiryPollAt = -Infinity;
    this.lifecycle = new SessionLifecycle(repository, (id, action) => this.serialize(id, action), session => this.removeSession(session), () => this.changed(), async () => {
      expiryPollAt = Date.now();
      await this.tracker.poll?.();
    }, id => !this.tracker.poll || backend.live().filter(owned => owned.sessionId === id && owned.state === 'open').every(owned => {
      const observation = this.observations.get(owned.tabId);
      return observation?.generation === owned.generation && observation.root === 'alive' && observation.health === 'healthy' && Date.parse(observation.observedAt) >= expiryPollAt;
    }));
    this.tracker = trackerFactory(items => {
      if (this.disposed) return;
      let publicChanged = false;
      const reason = (observation?: TabObservation) => observation
        ? observation.reason ?? (observation.agents > 0 ? 'A matched agent process is running in this shell.' : 'The shell is alive; no matched agent process is running.')
        : 'Awaiting owned shell process evidence.';
      for (const owned of backend.live()) backend.rememberOwnership(owned.tabId, owned.generation, this.tracker.resolveIdentity?.(owned.root) ?? null, this.tracker.resolveDescendants?.(owned.root) ?? []);
      for (const item of items) {
        const tab = repository.tab(item.tabId), owned = backend.get(item.tabId);
        if (tab?.terminal && owned?.state === 'open' && owned.generation === tab.operationId && tab.sessionId === item.sessionId) {
          const previous = this.observations.get(tab.id);
          const current = previous?.generation === owned.generation ? previous : undefined;
          // Compare public fields, not poll timestamps or a serialized session/snapshot.
          publicChanged ||= !current || tabStatus(tab.lifecycle, tab.error, current) !== tabStatus(tab.lifecycle, tab.error, item)
            || (current.root === 'alive' ? current.agents : 0) !== (item.root === 'alive' ? item.agents : 0)
            || tab.lifecycle !== 'closed' && reason(current) !== reason(item);
          const descendants = this.tracker.resolveDescendants?.(owned.root) ?? [];
          const key = owned.generation + '/' + processActivityKey(item) + '/' + descendants.map(p => p.domain + ':' + p.pid + ':' + p.birth).sort().join(',');
          if (this.processKeys.get(tab.id) !== key) this.lifecycle.touch(tab.sessionId);
          this.processKeys.set(tab.id, key);
          this.observations.set(tab.id, { ...item, generation: owned.generation });
        }
      }
      if (publicChanged) this.changed('native');
    });
    this.visibility.setLocal([]);
    backend.configureGuestTermination(async (root, captured, known) => {
      // Explicit cleanup polls all owned roots; automatic status polls are viewport-scoped.
      await this.tracker.poll?.();
      await closeGuest(root, captured ?? this.tracker.resolveIdentity?.(root) ?? null, undefined, [...known, ...(this.tracker.resolveDescendants?.(root) ?? [])]);
    });
  }
  subscribe(listener: (event: ChangedEvent) => void): () => void { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; }
  subscribeTerminal(listener: (event: TerminalEvent) => void): () => void { this.streams.add(listener); return () => { this.streams.delete(listener); }; }
  selectSession(sessionId: string): void { if (this.repository.session(sessionId)) this.lifecycle.touch(sessionId); this.changed('sessions', sessionId); }
  private changed(reason: ChangedEvent['reason'] = 'sessions', selectSessionId?: string): void {
    const event: ChangedEvent = { revision: ++this.revision, reason, ...(selectSessionId ? { selectSessionId } : {}) };
    for (const listener of this.listeners) { try { listener(event); } catch { /* Renderer failures do not roll back ownership. */ } }
  }
  private serialize<T>(id: string, action: () => Promise<Result<T>>): Promise<Result<T>> {
    const next = (this.queues.get(id) ?? Promise.resolve()).catch(() => undefined).then(() => this.disposed || this.updateClosing ? failure('NATIVE_UNAVAILABLE', 'Shellfox is shutting down.') : action()).catch(() => failure('STORAGE_FAILED', 'The embedded operation could not be saved.', true)) as Promise<Result<T>>;
    this.queues.set(id, next);
    void next.finally(() => { if (this.queues.get(id) === next) this.queues.delete(id); });
    return next;
  }
  async initialize(_input?: NativeInit): Promise<void> {
    this.unsubscribe = this.backend.subscribe(event => { this.terminalControl.event(event); this.terminalEvent(event); });
    this.unsubscribeTitle = this.backend.subscribeTitle(event => {
      if (this.disposed) return;
      const owned = this.backend.get(event.tabId), tab = this.repository.tab(event.tabId);
      if (owned?.generation === event.generation && tab?.operationId === event.generation && tab.terminal?.userTitle === null) this.changed();
    });
    this.backend.configurePythonPath(this.repository.settings().pythonPath ?? null);
    // Drain even unexpected rejections before returning, so no startup mutation
    // outlives initialize. Result failures still feed the probe and migrations.
    const readiness = [this.backend.initialize(), this.integrations.initialize(), getProcessTrackingCapability(this.helperOptions)] as const;
    await Promise.allSettled(readiness);
    const [result, , tracking] = await Promise.all(readiness);
    const profiles = this.backend.getProfiles();
    const available = result.ok && profiles.profiles.some(p => p.available);
    this.probe = { ...this.probe, available, platform: process.platform, arch: process.arch,
      ...(process.platform === 'linux' ? { python: this.backend.getPython() } : {}),
      shells: profiles.profiles.filter(p => p.environment === 'local' && !p.elevation).map(p => ({ id: shellId(p), executable: p.executable, available: p.available, reason: null })),
      reasons: [LIFETIME_NOTICE, ...(result.ok ? [] : [result.error.message]), ...profiles.profiles.filter(p => !p.available).map(p => p.unavailableReason ?? `${p.label} is unavailable.`), ...(tracking.reason ? [tracking.reason] : [])],
      capabilities: { createWindow: available, addTab: available, focusWindow: available, activateTab: available, closeTerminal: available, splitPane: false, attachExisting: false, commandExitStatus: false, processTracking: tracking.available, explorerContextMenu: this.explorer.supported, embeddedTerminal: true, terminalLifetime: 'app-owned', shellSurvival: false } };
    this.repository.transaction(() => {
      // Saved PIDs cannot restore app-owned handles. Retired external roots are
      // metadata only, so discard their obsolete tab records without touching processes.
      for (const tab of this.repository.tabs()) this.repository.deleteTab(tab.id);
      // Removing obsolete terminal handles is not session inactivity. Preserve
      // settlement and last activity; the lifecycle policy owns automatic expiry.
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
    this.refreshWatch();
    this.integrations.markReady();
    this.lifecycle.start();
    this.changed('native');
  }
  private refreshWatch(): void {
    const live = this.backend.live();
    // Adding/closing another tab does not invalidate evidence for an unchanged
    // shell generation. Drop closed/replaced roots, not the entire session.
    for (const [id, observation] of this.observations) if (!live.some(e => e.tabId === id && e.state === 'open' && e.generation === observation.generation)) { this.observations.delete(id); this.processKeys.delete(id); }
    this.tracker.setWatch(live.map(e => e.root), this.repository.settings().processRules);
  }
  private terminalEvent(event: TerminalEvent): void {
    const owned = this.backend.get(event.tabId);
    if (!owned || event.type !== 'activity' && owned.generation !== event.generation) return;
    // Runtime identity, not SQLite availability, authorizes this stream. Forward before persistence.
    this.forwardTerminal(event);
    if (event.type === 'data' || event.type === 'exit') this.lifecycle.touch(owned.sessionId);
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
        if (tab?.terminal && tab.operationId === exit.generation) this.repository.deleteTab(tab.id);
        this.pendingExits.delete(id); this.reportedExitFailures.delete(id); this.processKeys.delete(id);
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
      // Only explicitly automatic tabs accept titles from the matching runtime generation.
      const title = tab.terminal?.userTitle === null && owned?.generation === tab.operationId
        ? owned.processTitle ?? tab.title : tab.terminal?.userTitle ?? tab.title;
      return { id: tab.id, sessionId: tab.sessionId, title, cwd: tab.cwd, ordinal: tab.ordinal, createdAt: tab.createdAt,
        lifecycle, status: tabStatus(lifecycle, tab.error, embedded ? observation : undefined),
        agents: embedded && owned?.state === 'open' && observation?.root === 'alive' ? observation.agents : 0,
        monitoringReason: !embedded ? 'Legacy external terminal metadata only. Its process is not adopted, watched or terminated.' : lifecycle === 'closed' ? owned?.cleanupPending ? 'The root shell exited, but owned descendant cleanup is still pending. No replacement or complete shutdown is claimed.' : exit ? 'Runtime shell exit is confirmed. Saved closure metadata is being retried.' : 'The app-owned shell is closed. Commands are not restored.' : observation ? observation.reason ?? (observation.agents > 0 ? 'A matched agent process is running in this shell.' : 'The shell is alive; no matched agent process is running.') : 'Awaiting owned shell process evidence.',
        error: tab.error, generation: tab.operationId, terminalKind: embedded ? 'embedded' : 'external-legacy', profileId: tab.terminal?.profileId ?? null, exitCode: exit?.generation === tab.operationId ? exit.exitCode : owned?.generation === tab.operationId ? owned.exitCode : tab.terminal?.exitCode ?? null };
    });
    const embedded = session.adapterId === 'embedded-pty', live = this.backend.live().some(e => e.sessionId === session.id);
    const activity = aggregateStatus(tabs, session.error);
    return { id: session.id, title: session.title, cwd: session.cwd, adapterId: session.adapterId, shellId: session.shellId, createdAt: session.createdAt, updatedAt: session.updatedAt, settledAt: session.settledAt, pinnedAt: session.pinnedAt ?? null,
      status: publicStatus(activity, session.settledAt), activityStatus: activity, counts: countTabs(tabs), tabs, env: session.env ?? [], canFocus: live,
      canAddTab: this.probe.available, controlReason: embedded ? LIFETIME_NOTICE : 'Legacy external terminals are not controlled. Activating this task starts a separate embedded shell.', error: session.error,
      terminalLifetime: embedded ? 'app-owned' : 'external-legacy', shellSurvival: false,
      window: { state: 'unsupported', canReopen: false, canRegister: false, reason: 'Embedded tabs do not use native window ownership or external shell registration.' } };
  }
  getSnapshot(): Result<ManagerSnapshot> { return success({ revision: this.revision, sessions: sortSessions(this.repository.sessions().map(s => this.toDto(s))), settings: this.repository.settings(), probe: this.probe, explorer: this.explorer, cli: this.cli }); }
  private stats?: ReturnType<typeof createSessionStatsCollector>;
  async getSessionStats(input: { sessionId?: string }): Promise<Result<SessionStatsDto>> {
    const parsed = requestSchemas.getSessionStats.safeParse(input);
    if (!parsed.success) return failure('VALIDATION', 'Invalid session stats request.');
    if (this.disposed || this.updateClosing) return failure('NATIVE_UNAVAILABLE', 'Shellfox is shutting down.');
    const snapshot = this.getSnapshot(); if (!snapshot.ok) return snapshot;
    const sessions = snapshot.value.sessions.filter(session => !parsed.data.sessionId || session.id === parsed.data.sessionId).map(session => ({ ...session, env: [] }));
    if (parsed.data.sessionId && !sessions.length) return failure('NOT_FOUND', 'Session not found.');
    const ids = new Set(sessions.map(session => session.id));
    this.stats ??= createSessionStatsCollector(createSystemSnapshotProvider(this.helperOptions));
    try {
      const tabs = await this.stats(this.backend.live().filter(tab => ids.has(tab.sessionId)));
      return this.disposed || this.updateClosing ? failure('NATIVE_UNAVAILABLE', 'Shellfox is shutting down.') : success({ sessions, tabs });
    } catch { return failure('MONITOR_UNAVAILABLE', 'Owned process statistics could not be collected.', true); }
  }
  /** CLI filters need fresh evidence for hidden sessions as well as visible ones.
   * Explicit poll() covers the lifetime watch without changing renderer interest. */
  async getControlSnapshot(): Promise<Result<ManagerSnapshot>> {
    if (this.disposed || this.updateClosing) return failure('NATIVE_UNAVAILABLE', 'Shellfox is shutting down.');
    try { await this.tracker.poll?.(); }
    catch { return failure('MONITOR_UNAVAILABLE', 'Session status could not be refreshed.'); }
    if (this.disposed || this.updateClosing) return failure('NATIVE_UNAVAILABLE', 'Shellfox is shutting down.');
    return this.getSnapshot();
  }
  getTerminalProfiles(): Result<TerminalProfilesDto> { return success(this.backend.getProfiles()); }
  attachTerminal(input: Parameters<PtyBackend['attach']>[0]): Result<TerminalAttachmentDto> {
    const tab = this.repository.tab(input.tabId);
    if (!tab?.terminal) return failure('UNSUPPORTED', 'Only owned embedded terminals can be attached.');
    return this.backend.attach(input);
  }
  writeTerminal(input: Parameters<PtyBackend['write']>[0]): Result<{ written: true }> {
    const result = this.backend.write(input), owned = this.backend.get(input.tabId);
    if (result.ok && input.userInitiated !== false && input.data.length && owned) {
      this.lifecycle.touch(owned.sessionId);
      try {
        const session = this.repository.session(owned.sessionId);
        if (session?.settledAt) {
          session.settledAt = null;
          session.updatedAt = new Date().toISOString();
          this.repository.saveSession(session);
          this.changed();
        }
      } catch {
        // Input already reached the PTY. Do not report a failed write that a
        // client might repeat; later input can retry clearing the marker.
      }
    }
    return result;
  }
  resizeTerminal(input: Parameters<PtyBackend['resize']>[0]): Result<{ resized: true }> { return this.backend.resize(input); }
  private profile(cwd: string, session?: SessionRecord, explicit?: string): TerminalProfileDto | undefined {
    const profiles = this.backend.getProfiles(), settings = this.repository.settings();
    if (explicit !== undefined) return profiles.profiles.find(p => p.id === explicit && p.available);
    if (process.platform === 'win32') {
      const unc = parseWslUnc(cwd);
      if (unc) return profiles.profiles.find(p => p.environment === 'wsl' && p.distro?.toLowerCase() === unc.distro.toLowerCase() && p.available);
      // A global WSL preference or an old WSL tab must not route a Windows folder into WSL.
      const configured = profiles.profiles.find(p => p.id === settings.terminalProfileId && p.environment === 'local' && !p.elevation);
      if (configured) return configured.available ? configured : undefined;
      return profiles.profiles.find(p => p.id === 'pwsh' && p.available)
        ?? profiles.profiles.find(p => p.id === 'windows-powershell' && p.available)
        ?? profiles.profiles.find(p => p.environment === 'local' && p.available && !p.elevation);
    }
    const existing = session && this.repository.tabs(session.id).find(t => t.terminal)?.terminal?.profileId;
    const id = existing ?? settings.terminalProfileId ?? profiles.defaultProfileId;
    return profiles.profiles.find(p => p.id === id && p.available && !p.elevation);
  }
  createSession(input: { cwd: string; requestId: string; title?: string }, creator?: TerminalControlView): Promise<Result<SessionDto>> {
    if (!requestSchemas.createSession.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid session request.'));
    // Only duplicate requests share a queue. A slow guest startup must not
    // hold unrelated new sessions behind its directory/preflight waits.
    return this.serialize(`create:${input.requestId}`, async () => {
      const existing = this.repository.sessionByRequest(input.requestId); if (existing) return success(this.toDto(existing));
      const profile = this.profile(input.cwd); if (!profile || !this.probe.available) return failure('DEPENDENCY_MISSING', 'No discovered interactive terminal profile is available.');
      if (this.repository.sessions().length >= 10000) return failure('VALIDATION', 'The saved session limit is reached.');
      const now = new Date().toISOString();
      const session: SessionRecord = { id: randomUUID(), title: input.title ?? (path.basename(input.cwd).slice(0, 200) || input.cwd.slice(0, 200)), cwd: input.cwd, adapterId: 'embedded-pty', shellId: shellId(profile), shellExecutable: profile.executable, createdAt: now, updatedAt: now, settledAt: null, error: null, target: null, binding: null, windowState: 'unknown' };
      return this.open(session, profile, 'create', input.requestId, undefined, undefined, creator);
    });
  }
  activateSession(input: { sessionId: string }, creator?: TerminalControlView): Promise<Result<SessionDto>> {
    if (!requestSchemas.activateSession.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid session ID.'));
    return this.serialize(input.sessionId, async () => {
      const session = this.repository.session(input.sessionId); if (!session) return failure('NOT_FOUND', 'Session not found.');
      this.lifecycle.touch(session.id);
      if (this.backend.live().some(e => e.sessionId === session.id && e.state === 'open') || this.repository.tabs(session.id).some(t => t.terminal && t.lifecycle === 'launching')) return success(this.toDto(session));
      const cleanup = await this.backend.awaitCleanup(session.id); if (!cleanup.ok) return cleanup;
      const profile = this.profile(session.cwd, session); if (!profile || !this.probe.available) return failure('DEPENDENCY_MISSING', 'The session terminal profile is unavailable.');
      return this.open(session, profile, 'reopen', null, undefined, undefined, creator);
    });
  }
  addTab(input: { sessionId: string; title?: string; profileId?: string; cwd?: string }, creator?: TerminalControlView): Promise<Result<SessionDto>> {
    if (!requestSchemas.addTab.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid new tab request.'));
    return this.serialize(input.sessionId, async () => {
      const session = this.repository.session(input.sessionId); if (!session) return failure('NOT_FOUND', 'Session not found.');
      const profile = this.profile(input.cwd ?? session.cwd, session, input.profileId); if (!profile || !this.probe.available) return failure('DEPENDENCY_MISSING', 'The selected terminal profile is unavailable.');
      return this.open(session, profile, 'add', null, input.title, input.cwd, creator);
    });
  }
  private async open(session: SessionRecord, profile: TerminalProfileDto, kind: OperationRecord['kind'], requestId: string | null = null, title?: string, cwd?: string, creator?: TerminalControlView): Promise<Result<SessionDto>> {
    const tabs = this.repository.tabs(session.id);
    if (tabs.length >= 1000 || this.backend.live().length >= 64) return failure('VALIDATION', 'The embedded terminal limit is reached.');
    const ordinal = tabs.reduce((n, t) => Math.max(n, t.ordinal), -1) + 1, now = new Date().toISOString();
    const tab: TabRecord = { id: randomUUID(), sessionId: session.id, title: title ?? (profile.elevation ? profile.label : `Shell ${ordinal + 1}`), cwd: cwd ?? session.cwd, ordinal, createdAt: now, lifecycle: 'launching', operationId: randomUUID(), registration: null, error: null, terminal: { kind: 'embedded', profileId: profile.id, exitCode: null, userTitle: title ?? null } };
    const operation: OperationRecord = { id: tab.operationId, sessionId: session.id, tabId: tab.id, requestId, kind, state: 'intent', createdAt: now, updatedAt: now, error: null };
    // Reserve before publishing intent or starting the PTY. Eager viewers may
    // attach during launch, before the creator receives its response.
    const identity = { tabId: tab.id, sessionId: session.id, generation: tab.operationId };
    if (creator) { const reserved = this.terminalControl.reserve(creator, identity); if (!reserved.ok) return reserved; }
    let openedForCreator = false;
    try {
      this.repository.transaction(() => { this.repository.saveSession(session); this.repository.saveTab(tab); this.repository.saveOperation(operation); }); this.changed();
      const launched = await this.backend.launch({ tabId: tab.id, sessionId: session.id, generation: tab.operationId, profileId: profile.id, cwd: tab.cwd, env: this.repository.session(session.id)?.env ?? session.env ?? [], ...(this.cli.installed && this.cliIntegration ? { cliBin: this.cliIntegration.binDir } : {}) });
      try {
        this.repository.transaction(() => {
          // An immediate PTY exit may already have removed this tab.
          const current = this.repository.tab(tab.id);
          const latest = this.repository.session(session.id)!;
          if (!launched.ok) {
            const uncertain = this.backend.get(tab.id)?.state === 'open';
            if (current) { current.lifecycle = uncertain ? 'launch-uncertain' : 'closed'; current.error = launched.error; }
            operation.state = uncertain ? 'uncertain' : 'failed'; operation.error = launched.error;
            latest.error = launched.error;
          } else {
            if (current) { current.cwd = process.platform === 'win32' && parseWslUnc(tab.cwd) ? tab.cwd : launched.value.cwd; current.lifecycle = launched.value.state === 'open' ? 'open' : 'closed'; current.terminal!.exitCode = launched.value.exitCode; }
            operation.state = 'registered';
            latest.error = null;
            latest.adapterId = 'embedded-pty'; latest.shellId = shellId(profile); latest.shellExecutable = profile.executable; latest.target = null; latest.binding = null;
          }
          latest.updatedAt = new Date().toISOString(); operation.updatedAt = latest.updatedAt;
          if (current && current.lifecycle !== 'closed') { this.repository.saveTab(current); this.repository.saveOperation(operation); }
          else this.repository.deleteTab(tab.id);
          this.repository.saveSession(latest);
        });
      } catch {
        if (launched.ok) await this.backend.close({ tabId: tab.id, generation: tab.operationId });
        throw new Error('Ownership persistence failed.');
      }
      this.refreshWatch(); this.changed();
      // Persist the session/error and any uncertain owned handle, but report
      // every failed launch to the caller instead of claiming a shell was opened.
      if (!launched.ok) return launched;
      const opened = this.repository.tab(tab.id), owned = this.backend.get(tab.id);
      if (!opened || opened.operationId !== tab.operationId || opened.lifecycle !== 'open' || owned?.generation !== tab.operationId || owned.state !== 'open') {
        return failure('TARGET_LOST', 'The new shell exited before it could be opened.');
      }
      const result = success(this.toDto(this.repository.session(session.id)!));
      openedForCreator = true;
      return result;
    } finally {
      if (creator && !openedForCreator) creator.detach({ tabId: tab.id, generation: tab.operationId });
    }
  }
  closeTab(input: { tabId: string; generation: string }): Promise<Result<SessionDto>> {
    if (!requestSchemas.closeTab.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid close request.'));
    const initial = this.repository.tab(input.tabId); if (!initial) return Promise.resolve(failure('NOT_FOUND', 'Tab not found.'));
    if (!initial.terminal) return Promise.resolve(failure('UNSUPPORTED', 'Legacy external processes are not owned by this manager.'));
    if (initial.operationId !== input.generation) return Promise.resolve(failure('TARGET_LOST', 'This terminal generation is stale.'));
    // The add/reopen operation may be waiting for UAC in this very session's
    // queue. Interrupt its exact startup now, not inside the queued close.
    const interruption = this.backend.cancelStartup(input);
    return this.serialize(initial.sessionId, async () => {
      const interrupted = await interruption;
      if (interrupted && !interrupted.ok) return interrupted;
      const tab = this.repository.tab(input.tabId);
      if (!tab) {
        const session = this.repository.session(initial.sessionId);
        return session ? success(this.toDto(session)) : failure('NOT_FOUND', 'Session not found.');
      }
      if (!tab.terminal) return failure('UNSUPPORTED', 'Legacy external processes are not owned by this manager.');
      if (tab.operationId !== input.generation) return failure('TARGET_LOST', 'This terminal generation is stale.');
      if (tab.lifecycle === 'closed' && !this.backend.get(tab.id)) {
        this.repository.deleteTab(tab.id); this.changed(); return success(this.toDto(this.repository.session(tab.sessionId)!));
      }
      const closed = interrupted ?? await this.backend.close(input); if (!closed.ok) return closed;
      // The PTY exit event is authoritative. A pending OS close must not authorize a replacement shell.
      this.reconcileExits();
      if (this.pendingExits.has(tab.id)) return failure('STORAGE_FAILED', 'Closed tab removal could not be saved. Retry after storage is available.', true);
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
      s.pinnedAt = input.pinned ? s.pinnedAt ?? new Date().toISOString() : null;
    });
  }
  renameSession(input: { sessionId: string; title: string }): Promise<Result<SessionDto>> {
    if (!requestSchemas.renameSession.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid title.'));
    return this.mutate(input.sessionId, s => { s.title = input.title; });
  }
  renameTab(input: { sessionId: string; tabId: string; title: string }): Promise<Result<SessionDto>> {
    const parsed = requestSchemas.renameTab.safeParse(input);
    if (!parsed.success) return Promise.resolve(failure('VALIDATION', 'Invalid tab title.'));
    return this.serialize(parsed.data.sessionId, async () => {
      const session = this.repository.session(parsed.data.sessionId);
      if (!session) return failure('NOT_FOUND', 'Session not found.');
      const tab = this.repository.tab(parsed.data.tabId);
      if (!tab || tab.sessionId !== session.id) return failure('NOT_FOUND', 'Tab not found in this session.');
      if (!tab.terminal) return failure('UNSUPPORTED', 'External tabs are read-only.');
      this.repository.transaction(() => {
        tab.title = parsed.data.title.trim();
        tab.terminal!.userTitle = tab.title;
        this.repository.saveTab(tab);
        session.updatedAt = new Date().toISOString(); this.repository.saveSession(session);
      });
      this.changed(); return success(this.toDto(session));
    });
  }
  reorderTabs(input: { sessionId: string; tabIds: string[] }): Promise<Result<SessionDto>> {
    const parsed = requestSchemas.reorderTabs.safeParse(input);
    if (!parsed.success) return Promise.resolve(failure('VALIDATION', 'Invalid tab order.'));
    return this.serialize(parsed.data.sessionId, async () => {
      const session = this.repository.session(parsed.data.sessionId);
      if (!session) return failure('NOT_FOUND', 'Session not found.');
      const tabs = this.repository.tabs(session.id), byId = new Map(tabs.map(tab => [tab.id, tab]));
      if (tabs.length !== parsed.data.tabIds.length || parsed.data.tabIds.some(id => !byId.has(id))) return failure('VALIDATION', 'Tab order must contain every saved tab in this session exactly once. Refresh and try again.');
      if (!tabs.some(tab => tab.terminal)) return failure('UNSUPPORTED', 'External tabs are read-only.');
      this.repository.transaction(() => {
        // SQLite checks UNIQUE(sessionId, ordinal) on each write. Stage above the
        // current maximum before assigning dense ordinals, all in one transaction.
        const offset = Math.max(...tabs.map(tab => tab.ordinal)) + 1;
        tabs.forEach((tab, index) => this.repository.saveTab({ ...tab, ordinal: offset + index }));
        parsed.data.tabIds.forEach((id, ordinal) => this.repository.saveTab({ ...byId.get(id)!, ordinal }));
        session.updatedAt = new Date().toISOString(); this.repository.saveSession(session);
      });
      this.changed(); return success(this.toDto(session));
    });
  }
  private async closeSessionTerminals(id: string): Promise<Result<{ closed: true }>> {
    const tabs = this.repository.tabs(id);
    if (tabs.some(t => t.terminal && t.lifecycle === 'launching')) return failure('UNSUPPORTED', 'A terminal in this session is still starting. Try again in a moment.');
    for (const live of this.backend.live().filter(e => e.sessionId === id)) {
      const closed = await this.backend.close({ tabId: live.tabId, generation: live.generation });
      if (!closed.ok) return closed;
    }
    if (this.backend.live().some(e => e.sessionId === id)) return failure('TARGET_LOST', 'Session terminal closure is not confirmed.');
    this.reconcileExits();
    if (tabs.some(t => this.pendingExits.has(t.id))) return failure('STORAGE_FAILED', 'Closed tab removal could not be saved. Retry after storage is available.', true);
    return success({ closed: true });
  }
  settleSession(input: { sessionId: string; confirmActive: boolean }): Promise<Result<SessionDto>> {
    if (!requestSchemas.settleSession.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid settle request.'));
    return this.mutate(input.sessionId, s => { s.settledAt ??= new Date().toISOString(); });
  }
  unsettleSession(input: { sessionId: string }): Promise<Result<SessionDto>> {
    if (!requestSchemas.unsettleSession.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid session ID.'));
    return this.mutate(input.sessionId, s => { s.settledAt = null; });
  }
  private async removeSession(session: SessionRecord): Promise<Result<{ deleted: true }>> {
    const tabs = this.repository.tabs(session.id);
    const closed = await this.closeSessionTerminals(session.id); if (!closed.ok) return closed;
    if (!this.repository.deleteSession(session.id)) return failure('NOT_FOUND', 'Session not found.');
    for (const tab of tabs) { this.observations.delete(tab.id); this.processKeys.delete(tab.id); }
    this.lifecycle.forget(session.id);
    this.changed(); return success({ deleted: true });
  }
  deleteSession(input: { sessionId: string }): Promise<Result<{ deleted: true }>> {
    if (!requestSchemas.deleteSession.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'Invalid session ID.'));
    return this.serialize(input.sessionId, async () => {
      const session = this.repository.session(input.sessionId);
      return session ? this.removeSession(session) : failure('NOT_FOUND', 'Session not found.');
    });
  }
  /** CLI mutations share the renderer's session queue, but never enqueue another
   * session operation while holding it. Context is checked again after waiting. */
  controlSession(input: ControlRequest, signal?: AbortSignal): Promise<Result<ControlResponse>> {
    const parsed = controlRequestSchema.safeParse(input);
    if (!parsed.success || !['inspect', 'settle', 'unsettle', 'delete', 'tab-open', 'tab-close'].includes(parsed.data.action)) return Promise.resolve(failure('VALIDATION', 'Invalid session control request.'));
    input = parsed.data; // Capture validated values before waiting in the queue.
    if (input.action === 'delete' && input.confirmed !== true) return Promise.resolve(failure('SETTLE_CONFIRM_REQUIRED', 'Explicit confirmation is required to close all session terminals.'));
    const resolveContext = () => {
      const matches = this.backend.live().filter(e => e.state === 'open' && e.root.marker === input.marker);
      if (matches.length !== 1) return undefined;
      const owned = matches[0]!, tab = this.repository.tab(owned.tabId);
      return tab?.terminal && tab.sessionId === owned.sessionId && tab.operationId === owned.generation ? owned : undefined;
    };
    const context = input.id ? undefined : resolveContext();
    let id: string;
    if (input.id) {
      const exact = this.repository.session(input.id);
      const matches = exact ? [exact] : this.repository.sessions().filter(s => s.id.toLowerCase().startsWith(input.id!.toLowerCase()));
      if (!matches.length) return Promise.resolve(failure('NOT_FOUND', 'Session ID not found.'));
      if (matches.length !== 1) return Promise.resolve(failure('TARGET_AMBIGUOUS', 'Session ID prefix is ambiguous. Use the full ID.'));
      id = matches[0]!.id;
    } else {
      if (!context) return Promise.resolve(failure('TARGET_LOST', 'Live terminal context not found. Run this command in an owned Shellfox terminal.'));
      id = context.sessionId;
    }
    return this.serialize<ControlResponse>(id, async () => {
      if (signal?.aborted) return failure('TARGET_LOST', 'The control request was cancelled before execution.');
      const session = this.repository.session(id);
      if (!session) return failure('NOT_FOUND', 'Session not found.');
      if (context) {
        const current = resolveContext();
        if (!current || current.sessionId !== id || current.tabId !== context.tabId || current.generation !== context.generation) return failure('TARGET_LOST', 'The invoking terminal context is no longer live.');
      }
      if (input.action === 'inspect') return success({ session: this.toDto(session) });
      if (input.action === 'tab-open') {
        if (session.adapterId !== 'embedded-pty') return failure('UNSUPPORTED', 'Legacy external sessions are not controlled.');
        const profile = this.profile(session.cwd, session);
        if (!profile || !this.probe.available) return failure('DEPENDENCY_MISSING', 'The session terminal profile is unavailable.');
        const opened = await this.open(session, profile, 'add');
        return opened.ok ? success({ session: opened.value }) : opened;
      }
      if (input.action === 'tab-close') {
        const closed = await this.backend.close({ tabId: context!.tabId, generation: context!.generation });
        if (!closed.ok) return closed;
        this.reconcileExits();
        if (this.pendingExits.has(context!.tabId)) return failure('STORAGE_FAILED', 'Closed tab removal could not be saved. Retry after storage is available.', true);
        this.changed(); return success({ session: this.toDto(session) });
      }
      if (input.action === 'delete') {
        const removed = await this.removeSession(session);
        return removed.ok ? success({ message: 'Session deleted.' }) : removed;
      }
      session.settledAt = input.action === 'settle' ? session.settledAt ?? new Date().toISOString() : null;
      session.updatedAt = new Date().toISOString();
      this.repository.saveSession(session); this.changed();
      return success({ session: this.toDto(session), message: input.action === 'settle' ? 'Session settled.' : 'Session unsettled.' });
    });
  }
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
      const selected = profiles.profiles.find(p => p.id === requested && p.available && !p.elevation);
      if (!selected) { if (pythonChanged) await this.backend.refreshProfiles(previous.pythonPath ?? null); return failure('VALIDATION', 'Choose a discovered terminal profile.'); }
      // An explicit discovered profile owns its derived fields. Full DTOs may still carry the old executable.
      const unchangedExecutable = !!input.terminalProfileId && input.shellExecutable === previous.shellExecutable;
      if (input.shellExecutable && input.shellExecutable !== selected.executable && !unchangedExecutable) { if (pythonChanged) await this.backend.refreshProfiles(previous.pythonPath ?? null); return failure('VALIDATION', 'Arbitrary shell executables are not allowed.'); }
      const settings: SettingsDto = { ...parsed.data, adapterId: 'embedded-pty', terminalProfileId: selected.id, shellId: shellId(selected), shellExecutable: selected.executable };
      try { this.observations.clear(); this.tracker.setWatch(this.backend.live().map(e => e.root), settings.processRules); this.repository.saveSettings(settings); }
      catch { if (pythonChanged) await this.backend.refreshProfiles(previous.pythonPath ?? null); this.tracker.setWatch(this.backend.live().map(e => e.root), previous.processRules); return failure('STORAGE_FAILED', 'Settings could not be applied.', true); }
      if (pythonChanged) {
        const available = profiles.profiles.some(p => p.available);
        this.probe = { ...this.probe, available, python: this.backend.getPython(), reasons: [LIFETIME_NOTICE, ...profiles.profiles.filter(p => !p.available).map(p => p.unavailableReason ?? 'Shell unavailable')], shells: profiles.profiles.filter(p => p.environment === 'local' && !p.elevation).map(p => ({ id: shellId(p), executable: p.executable, available: p.available, reason: p.unavailableReason ?? null })), capabilities: { ...this.probe.capabilities, createWindow: available, addTab: available, closeTerminal: available, focusWindow: available, activateTab: available } };
      }
      this.changed('settings'); return success(settings);
    });
  }
  refreshIntegrations(): Promise<void> { return this.integrations.refresh(); }
  setCliIntegration(input: { installed: boolean }) {
    if (!requestSchemas.setCliIntegration.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'CLI integration is always enabled.'));
    return this.integrations.ensureCli();
  }
  setExplorerIntegration(input: { installed: boolean }): Promise<Result<ExplorerIntegrationDto>> {
    if (!requestSchemas.setExplorerIntegration.safeParse(input).success) return Promise.resolve(failure('VALIDATION', 'File-manager integration is always enabled.'));
    return this.integrations.ensureExplorer();
  }
  ownedTerminalCount(): number { return this.backend.live().length + this.repository.tabs().filter(t => t.terminal && t.lifecycle === 'launching' && !this.backend.get(t.id)).length; }
  private async closeOwnedTerminals(): Promise<void> {
    // UAC/readiness waits are inside operation queues. Abort them before waiting
    // for those queues, while keeping every unconfirmed helper handle owned.
    await this.backend.cancelPendingStartups();
    await Promise.allSettled([...this.queues.values()]);
    await this.integrations.drain();
    const results = await Promise.all(this.backend.live().map(e => this.backend.close({ tabId: e.tabId, generation: e.generation })));
    if (results.some(r => !r.ok)) throw new Error('Owned terminal shutdown was not confirmed.');
    this.reconcileExits();
    if (this.pendingExits.size) throw new Error('Shells terminated, but exit metadata remains unsaved. Storage reconciliation is pending.');
  }
  /** Close shells without destroying the backend/tracker, so failed handoffs can resume. */
  async closeForUpdate(): Promise<() => void> {
    if (this.disposed || this.updateClosing) throw new Error('Terminal shutdown is already in progress.');
    this.updateClosing = true;
    const resume = () => { this.updateClosing = false; this.refreshWatch(); };
    try { await this.closeOwnedTerminals(); return resume; }
    catch (error) { resume(); throw error; }
  }
  async dispose(options: { fatal?: boolean } = {}): Promise<void> {
    if (this.disposed) return;
    this.disposed = true;
    this.lifecycle.stop();
    let fatalError: unknown;
    try {
      // Save confirmed exits before irreversibly disposing the backend. Storage
      // failures must leave a backend capable of opening fresh shells on retry.
      await this.closeOwnedTerminals();
      this.lifecycle.flushActivity();
      await this.backend.dispose();
    } catch (error) {
      if (!options.fatal) { this.disposed = false; this.lifecycle.start(); this.refreshWatch(); throw error; }
      fatalError = error;
    }
    this.terminalControl.dispose(); this.tracker.dispose();
    if (this.exitRetryTimer) clearInterval(this.exitRetryTimer);
    this.unsubscribe?.(); this.unsubscribeTitle?.(); this.listeners.clear(); this.streams.clear();
    if (fatalError !== undefined) throw fatalError;
  }
}
