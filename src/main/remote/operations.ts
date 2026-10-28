import { failure, success, type SessionDto } from '../../shared/contracts';
import { sessionSchema } from '../../shared/schemas';
import { requestSchemas, responseSchemas, remoteMethodSchema } from '../../shared/remote-schemas';
import type { EmbeddedSessionService } from '../terminal/service';
import { getHomeDirectory, resolveDirectory, createDirectory, completeDirectory } from '../directory-prompt';
import type { TerminalControlView } from '../terminal/control';
import { isTerminalUploadMethod } from '../../shared/terminal-upload-schemas';

export async function remoteOperation(service: EmbeddedSessionService, delivery: TerminalControlView, clientId: string, method: unknown, payload: unknown): Promise<unknown> {
  const parsedMethod = remoteMethodSchema.safeParse(method);
  if (!parsedMethod.success) return failure('AUTH_FAILED', 'This operation is not available remotely.');
  const m = parsedMethod.data;
  if (isTerminalUploadMethod(m)) return failure('UNSUPPORTED', 'Uploads require an authenticated remote host view.');
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
    case 'takeTerminalControl': tabId = requestSchemas.takeTerminalControl.parse(payload).tabId; break;
    case 'writeTerminal': tabId = requestSchemas.writeTerminal.parse(payload).tabId; break;
    case 'resizeTerminal': tabId = requestSchemas.resizeTerminal.parse(payload).tabId; break;
    case 'closeTab': tabId = requestSchemas.closeTab.parse(payload).tabId; break;
  }
  if (tabId && elevated.has(service.repository.tab(tabId)?.terminal?.profileId ?? '')) return failure('AUTH_FAILED', 'Elevated terminals are local-only.');
  if (m === 'deleteSession') {
    const p = requestSchemas.deleteSession.parse(payload);
    if (service.repository.tabs(p.sessionId).some(tab => elevated.has(tab.terminal?.profileId ?? ''))) return failure('AUTH_FAILED', 'Sessions containing elevated terminals can only be deleted locally.');

  }
  if (m === 'writeTerminal' || m === 'resizeTerminal' || m === 'closeTab') {
    const p = m === 'writeTerminal' ? requestSchemas.writeTerminal.parse(payload) : m === 'resizeTerminal' ? requestSchemas.resizeTerminal.parse(payload) : requestSchemas.closeTab.parse(payload);
    const owned = service.backend.get(p.tabId);
    if (!owned || owned.generation !== p.generation || service.repository.tab(p.tabId)?.operationId !== p.generation) return failure('TARGET_LOST', 'Stale terminal generation.');

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
    case 'attachTerminal': result = delivery.attach(requestSchemas.attachTerminal.parse(payload)); break;
    case 'takeTerminalControl': result = delivery.take(requestSchemas.takeTerminalControl.parse(payload)); break;
    case 'writeTerminal': result = delivery.write(requestSchemas.writeTerminal.parse(payload)); break;
    case 'resizeTerminal': result = delivery.resize(requestSchemas.resizeTerminal.parse(payload)); break;
    case 'closeTab': result = await service.closeTab(requestSchemas.closeTab.parse(payload)); break;
    case 'acknowledgeTerminal': result = delivery.acknowledge(requestSchemas.acknowledgeTerminal.parse(payload)); break;
    case 'detachTerminal': result = delivery.detach(requestSchemas.detachTerminal.parse(payload)); break;
    case 'createSession': result = await service.createSession(requestSchemas.createSession.parse(payload), delivery); break;
    case 'addTab': result = await service.addTab(requestSchemas.addTab.parse(payload), delivery); break;
    case 'activateSession': result = await service.activateSession(requestSchemas.activateSession.parse(payload), delivery); break;
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
