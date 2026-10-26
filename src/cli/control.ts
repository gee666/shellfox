import type { ControlCliIo } from '../main/control-cli';
import type { CliHost } from './execute';
import { CliFailure, unwrap } from './output';
import type { GlobalOptions } from './arguments';
import { resolveSessionId } from './session-context';
/** Fresh stats use one validated manager method locally and across an approved peer. */
export function managerControlRequest(host: CliHost, options: GlobalOptions): ControlCliIo['request'] {
  return async (userData, request) => {
    const peerId = options.host && options.host !== 'local' ? options.host : undefined;
    const sessionId = request.id ?? (request.current ? await resolveSessionId(undefined, userData, options, request.marker) : undefined);
    if (['list', 'stats', 'processes', 'inspect'].includes(request.action)) {
      const payload = sessionId ? { sessionId } : {};
      const response = unwrap(peerId ? await host.remote.request({ peerId, method: 'getSessionStats', payload }) : await host.request('getSessionStats', payload));
      if (request.action === 'inspect') {
        if (response.sessions.length !== 1) throw new CliFailure('NOT_FOUND', 'Use an explicit session ID to inspect one session.');
        return { session: response.sessions[0], tabs: response.tabs };
      }
      return response;
    }
    if (!sessionId) throw new CliFailure('VALIDATION', 'A session ID is required.');
    if (request.action === 'delete') {
      if (!request.confirmed) throw new CliFailure('VALIDATION', 'Deletion requires confirmation.');
      unwrap(peerId ? await host.remote.request({ peerId, method: 'deleteSession', payload: { sessionId } }) : await host.request('deleteSession', { sessionId }));
      return { message: 'Session deleted.' };
    }
    if (request.action === 'settle') return { session: unwrap(peerId ? await host.remote.request({ peerId, method: 'settleSession', payload: { sessionId, confirmActive: false } }) : await host.request('settleSession', { sessionId, confirmActive: false })), message: 'Session settled.' };
    if (request.action === 'unsettle') return { session: unwrap(peerId ? await host.remote.request({ peerId, method: 'unsettleSession', payload: { sessionId } }) : await host.request('unsettleSession', { sessionId })), message: 'Session unsettled.' };
    throw new CliFailure('UNSUPPORTED', 'Use the session or terminals namespace for this operation.');
  };
}
