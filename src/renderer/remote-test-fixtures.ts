import { vi } from 'vitest';
import { failure, success } from '../shared/contracts';
import type { RemoteApi, RemoteEvent, RemoteState } from '../shared/remote-contracts';
import { profiles, session, snapshot } from './test-fixtures';
import { responseSchemas } from '../shared/schemas';

export function remoteFixture() {
  const listeners = new Set<(event: RemoteEvent) => void>();
  const state: RemoteState = {
    enabled: true, hostname: 'local', status: 'online', error: null, loginUrl: null,
    peers: [
      { id: 'workstation', hostname: 'Workstation', addresses: ['100.64.0.1'], online: true, connection: 'connected', approved: true, remembered: true, hostConfirmed: true },
      { id: 'server', hostname: 'Linux server', addresses: ['100.64.0.2'], online: false, connection: 'disconnected', approved: true, remembered: true, hostConfirmed: true },
    ], approvals: [], pending: [],
  };
  const item = session(); item.title = 'Remote session'; item.cwd = '/home/remote/project';
  const remoteSnapshot = snapshot([item]); remoteSnapshot.probe.platform = 'linux';
  const disconnected: RemoteState = { ...state, peers: state.peers.map(peer => ({ ...peer, connection: 'disconnected' })) };
  const api: RemoteApi = {
    getState: vi.fn(async () => success(state)),
    configure: vi.fn(async () => success(state)),
    login: vi.fn(async () => success(state)),
    connect: vi.fn(async () => success(state)),
    disconnect: vi.fn(async () => success(disconnected)),
    approve: vi.fn(async () => success(state)),
    reject: vi.fn(async () => success(state)),
    revoke: vi.fn(async () => success(state)),
    forget: vi.fn(async () => success(state)),
    request: async () => { throw new Error('No mock response'); },
    subscribe: vi.fn(listener => { listeners.add(listener); return () => { listeners.delete(listener); }; }),
  };
  const request = vi.spyOn(api, 'request').mockResolvedValue(failure('UNSUPPORTED', 'Unavailable.'));
  request.mockResolvedValueOnce(responseSchemas.getSnapshot.parse(success(remoteSnapshot)))
    .mockResolvedValueOnce(responseSchemas.getTerminalProfiles.parse(success({ profiles, defaultProfileId: 'pwsh', lifetime: 'app-owned', shellSurvival: false })));
  function emit(event: RemoteEvent) { for (const listener of listeners) listener(event); }
  return { api, state, disconnected, request, remoteSnapshot, emit, listeners };
}
