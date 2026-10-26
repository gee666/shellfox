import { failure, success, type SessionDto } from '../../shared/contracts';
import { sessionSchema } from '../../shared/schemas';
import { requestSchemas, responseSchemas, remoteMethodSchema } from '../../shared/remote-schemas';
import type { EmbeddedSessionService } from '../terminal/service';
import { getHomeDirectory, resolveDirectory, createDirectory, completeDirectory } from '../directory-prompt';
import type { TerminalDelivery } from '../terminal/delivery';

/** Local input is never gated. Remote writers share a renewable 30 second control lease. */
export class RemoteControlLeases {
  private readonly owners = new Map<string, { clientId: string; until: number }>();
  claim(tabId: string, clientId: string, now = Date.now()): boolean { return this.claimAll([tabId], clientId, now); }
  claimAll(tabIds: string[], clientId: string, now = Date.now()): boolean {
    for (const [id, lease] of this.owners) if (lease.until <= now) this.owners.delete(id);
    if (tabIds.some(id => { const owner = this.owners.get(id); return owner && owner.clientId !== clientId; })) return false;
    for (const tabId of tabIds) this.owners.set(tabId, { clientId, until: now + 30_000 });
    return true;
  }
  release(clientId: string): void { for (const [tabId, owner] of this.owners) if (owner.clientId === clientId) this.owners.delete(tabId); }
}
export async function remoteOperation(service: EmbeddedSessionService, delivery: TerminalDelivery, leases: RemoteControlLeases, clientId: string, method: unknown, payload: unknown): Promise<unknown> {
  const parsedMethod = remoteMethodSchema.safeParse(method);
  if (!parsedMethod.success) return failure('AUTH_FAILED', 'This operation is not available remotely.');
  const m = parsedMethod.data;
  if (!requestSchemas[m].safeParse(payload).success) return failure('VALIDATION', 'Invalid remote request.');
  const profiles = service.getTerminalProfiles();
  if (!profiles.ok) return profiles;
  if (m === 'addTab') {
    const p = requestSchemas.addTab.parse(payload);
    if (p.profileId && !profiles.value.profiles.some(profile => profile.id === p.profileId && !profile.elevation && profile.available)) return failure('AUTH_FAILED', 'Remote terminals require a non-elevated profile.');
  }
  const elevated = new Set(profiles.value.profiles.filter(profile => profile.elevation).map(profile => profile.id));
  const filterSession = (session: SessionDto): SessionDto => {
    const tabs = session.tabs.filter(tab => !tab.profileId || !elevated.has(tab.profileId));
    return { ...session, tabs, counts: { running: tabs.filter(t => t.lifecycle !== 'closed' && t.status === 'running').length, waiting: tabs.filter(t => t.lifecycle !== 'closed' && t.status === 'waiting').length, unknown: tabs.filter(t => t.lifecycle !== 'closed' && t.status === 'unknown').length, error: tabs.filter(t => t.lifecycle !== 'closed' && t.status === 'error').length, closed: tabs.filter(t => t.lifecycle === 'closed').length, agents: tabs.reduce((sum, tab) => sum + tab.agents, 0) } };
  };
  // Elevated terminals stay local-only, including destructive session operations.
  let tabId: string | undefined;
  switch (m) {
    case 'attachTerminal': tabId = requestSchemas.attachTerminal.parse(payload).tabId; break;
    case 'writeTerminal': tabId = requestSchemas.writeTerminal.parse(payload).tabId; break;
    case 'resizeTerminal': tabId = requestSchemas.resizeTerminal.parse(payload).tabId; break;
    case 'closeTab': tabId = requestSchemas.closeTab.parse(payload).tabId; break;
  }
  if (tabId && elevated.has(service.repository.tab(tabId)?.terminal?.profileId ?? '')) return failure('AUTH_FAILED', 'Elevated terminals are local-only.');
  if (m === 'deleteSession') {
    const p = requestSchemas.deleteSession.parse(payload);
    if (service.repository.tabs(p.sessionId).some(tab => elevated.has(tab.terminal?.profileId ?? ''))) return failure('AUTH_FAILED', 'Sessions containing elevated terminals can only be deleted locally.');
    const tabs = service.backend.live().filter(tab => tab.sessionId === p.sessionId && tab.state === 'open').map(tab => tab.tabId);
    if (!leases.claimAll(tabs, clientId)) return failure('FOCUS_DENIED', 'Another remote client controls a terminal in this session. Disconnect it or wait for its control lease to expire before deleting the session.', true);
  }
  if (m === 'writeTerminal' || m === 'resizeTerminal' || m === 'closeTab') {
    const p = m === 'writeTerminal' ? requestSchemas.writeTerminal.parse(payload) : m === 'resizeTerminal' ? requestSchemas.resizeTerminal.parse(payload) : requestSchemas.closeTab.parse(payload);
    const owned = service.backend.get(p.tabId);
    if (!owned || owned.generation !== p.generation || service.repository.tab(p.tabId)?.operationId !== p.generation) return failure('TARGET_LOST', 'Stale terminal generation.');
    if (!leases.claim(p.tabId, clientId)) return failure('FOCUS_DENIED', 'Another remote client controls this terminal. Retry after its 30 second lease expires or it disconnects. Local input always remains available.', true);
  }
  let result: unknown;
  switch (m) {
    case 'getSnapshot': {
      const snapshot = service.getSnapshot();
      result = snapshot.ok ? success({ ...snapshot.value, sessions: snapshot.value.sessions.map(filterSession) }) : snapshot;
      break;
    }
    case 'getSessionStats': {
      const stats = await service.getSessionStats(requestSchemas.getSessionStats.parse(payload));
      if (!stats.ok) { result = stats; break; }
      const sessions = stats.value.sessions.map(session => ({ ...filterSession(session), env: [] }));
      const tabs = new Set(sessions.flatMap(session => session.tabs.map(tab => tab.id)));
      result = success({ sessions, tabs: stats.value.tabs.filter(tab => tabs.has(tab.tabId)) }); break;
    }
    case 'setVisibleSessions': result = service.setRemoteVisibleSessions(clientId, requestSchemas.setVisibleSessions.parse(payload)); break;
    case 'getTerminalProfiles': result = success({ ...profiles.value, profiles: profiles.value.profiles.filter(profile => !profile.elevation) }); break;
    case 'attachTerminal': {
      const attachment = service.attachTerminal(requestSchemas.attachTerminal.parse(payload));
      const accepted = attachment.ok ? delivery.attached(attachment.value) : null;
      result = accepted && !accepted.ok ? accepted : attachment; break;
    }
    case 'writeTerminal': result = service.writeTerminal(requestSchemas.writeTerminal.parse(payload)); break;
    case 'resizeTerminal': result = service.resizeTerminal(requestSchemas.resizeTerminal.parse(payload)); break;
    case 'closeTab': result = await service.closeTab(requestSchemas.closeTab.parse(payload)); break;
    case 'acknowledgeTerminal': result = delivery.acknowledge(requestSchemas.acknowledgeTerminal.parse(payload)); break;
    case 'detachTerminal': result = delivery.detach(requestSchemas.detachTerminal.parse(payload)); break;
    case 'createSession': result = await service.createSession(requestSchemas.createSession.parse(payload)); break;
    case 'addTab': result = await service.addTab(requestSchemas.addTab.parse(payload)); break;
    case 'activateSession': result = await service.activateSession(requestSchemas.activateSession.parse(payload)); break;
    case 'renameSession': result = await service.renameSession(requestSchemas.renameSession.parse(payload)); break;
    case 'renameTab': result = await service.renameTab(requestSchemas.renameTab.parse(payload)); break;
    case 'reorderTabs': result = await service.reorderTabs(requestSchemas.reorderTabs.parse(payload)); break;
    case 'setSessionPinned': result = await service.setSessionPinned(requestSchemas.setSessionPinned.parse(payload)); break;
    case 'settleSession': result = await service.settleSession(requestSchemas.settleSession.parse(payload)); break;
    case 'unsettleSession': result = await service.unsettleSession(requestSchemas.unsettleSession.parse(payload)); break;
    case 'deleteSession': result = await service.deleteSession(requestSchemas.deleteSession.parse(payload)); break;
    case 'clearSessionError': result = await service.clearSessionError(requestSchemas.clearSessionError.parse(payload)); break;
    case 'setSessionEnv': result = await service.setSessionEnv(requestSchemas.setSessionEnv.parse(payload)); break;
    case 'getHomeDirectory': result = getHomeDirectory(); break;
    case 'resolveDirectory': result = await resolveDirectory(requestSchemas.resolveDirectory.parse(payload)); break;
    case 'createDirectory': result = await createDirectory(requestSchemas.createDirectory.parse(payload)); break;
    case 'completeDirectory': result = await completeDirectory(requestSchemas.completeDirectory.parse(payload)); break;
  }
  const response = responseSchemas[m].safeParse(result);
  if (!response.success) return failure('INTERNAL', 'Invalid remote operation response.');
  if (response.data.ok) {
    const session = sessionSchema.safeParse(response.data.value);
    if (session.success) return success(filterSession(session.data));
  }
  return response.data;
}
