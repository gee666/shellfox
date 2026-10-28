import type { ManagerApi } from '../shared/contracts';
import { requestSchemas, responseSchemas, type ManagerMethod } from '../shared/schemas';
import { remoteForwardRequestSchema, remoteMethodSchema, remotePayloadSchemas, remoteRequestSchemas, responseSchemas as forwardedResponseSchemas } from '../shared/remote-schemas';
import { isTerminalUploadMethod } from '../shared/terminal-upload-schemas';
import { hostRequestSchemas, hostResponseSchemas } from '../shared/host-schemas';
import type { HostMethod } from '../shared/host-contracts';
import type { HostClient } from '../host/client';
import { isDestructive, type Command } from './commands';
import { CliUsageError, type GlobalOptions } from './arguments';
import { CliFailure, unwrap } from './output';
import { readAuthKey, readStdin, validateAuthKey } from './secrets';
import { attachTerminal, type AttachApi, type AttachIo } from './attach';
import { confirmControl } from '../main/control-cli';
import { resolveSessionId } from './session-context';
import { managerControlRequest } from './control';
import { colorEnabled } from './human';
import { formatControl } from '../main/ssh/presentation';
export type CliHost = Pick<HostClient, 'service' | 'remote' | 'request' | 'status' | 'stop' | 'close' | 'onDisconnect'>;
async function rawRequest(host: CliHost, method: HostMethod, payload: unknown): Promise<unknown> {
  return hostResponseSchemas[method].parse(await host.request(method, hostRequestSchemas[method].parse(payload)));
}
export async function requireConfirmation(options: GlobalOptions, description: string): Promise<void> {
  if (options.yes) return;
  if (options.json || !process.stdin.isTTY || !process.stdout.isTTY) throw new CliUsageError(description + ' requires --yes in noninteractive or JSON mode.');
  if (!await confirmControl(description + '? [y/N] ', process.stdin, process.stdout)) throw new CliFailure('CANCELLED', 'Cancelled. No changes made.');
}
type ManagerTarget = Pick<ManagerApi, 'getSnapshot' | 'resolveDirectory'> & Partial<AttachApi>;
export function managerTarget(host: CliHost, peerId?: string): ManagerTarget {
  if (!peerId || peerId === 'local') return host.service;
  const call = async <M extends ManagerMethod>(method: M, payload: unknown = {}) => {
    const forward = remoteForwardRequestSchema.parse({ peerId, method, payload });
    return rawRequest(host, 'remote.request', forward);
  };
  // Explicit methods keep terminal events scoped to this peer and never select a session.
  return {
    getSnapshot: async () => responseSchemas.getSnapshot.parse(await call('getSnapshot')),
    resolveDirectory: async input => responseSchemas.resolveDirectory.parse(await call('resolveDirectory', input)),
    attachTerminal: async input => responseSchemas.attachTerminal.parse(await call('attachTerminal', input)),
    takeTerminalControl: async input => responseSchemas.takeTerminalControl.parse(await call('takeTerminalControl', input)),
    writeTerminal: async input => responseSchemas.writeTerminal.parse(await call('writeTerminal', input)),
    resizeTerminal: async input => responseSchemas.resizeTerminal.parse(await call('resizeTerminal', input)),
    acknowledgeTerminal: async input => responseSchemas.acknowledgeTerminal.parse(await call('acknowledgeTerminal', input)),
    detachTerminal: async input => responseSchemas.detachTerminal.parse(await call('detachTerminal', input)),
    subscribeTerminal: listener => host.remote.subscribe(event => { if (event.type === 'terminal' && event.peerId === peerId) listener(event.event); }),
  };
}
async function request(host: CliHost, method: ManagerMethod, payload: unknown, peerId?: string): Promise<unknown> {
  const schema = peerId && peerId !== 'local' ? remotePayloadSchemas[method] : requestSchemas[method];
  const parsed = schema.safeParse(payload);
  if (!parsed.success) throw new CliUsageError('Invalid ' + method + ' input: ' + parsed.error.message);
  if (peerId && peerId !== 'local' && !remoteMethodSchema.safeParse(method).success) throw new CliFailure('UNSUPPORTED', method + ' is not available over a remote connection.');
  const result = peerId && peerId !== 'local'
    ? await rawRequest(host, 'remote.request', { peerId, method, payload: parsed.data })
    : await rawRequest(host, method, parsed.data);
  const response = responseSchemas[method].parse(result);
  if (!response.ok) throw new CliFailure(response.error.code, response.error.message, response.error.retryable);
  return response.value;
}
export async function executeCommand(command: Command, host: CliHost, options: GlobalOptions, attachIo?: AttachIo): Promise<unknown> {
  const service = managerTarget(host, options.host);
  switch (command.kind) {
    case 'control': throw new CliUsageError('Session metrics must use the control command runner.');
    case 'terminal-session': {
      const sessionId = await resolveSessionId(command.sessionId, options.userData ?? '', options);
      if (command.operation === 'list') return unwrap(await service.getSnapshot()).sessions.find(session => session.id === sessionId)?.tabs ?? [];
      const payload = { ...command.payload, sessionId };
      if (command.operation === 'create' && typeof command.payload.cwd === 'string') {
        const directory = unwrap(await service.resolveDirectory({ path: command.payload.cwd }));
        return request(host, 'addTab', { ...payload, cwd: directory.cwd }, options.host);
      }
      return request(host, command.operation === 'create' ? 'addTab' : command.operation === 'rename' ? 'renameTab' : 'reorderTabs', payload, options.host);
    }
    case 'session': {
      const sessionId = await resolveSessionId(command.sessionId, options.userData ?? '', options);
      if (command.operation === 'inspect' || (command.operation === 'env' && !('env' in command.payload))) {
        const selected = unwrap(await service.getSnapshot()).sessions.find(session => session.id === sessionId);
        if (!selected) throw new CliFailure('NOT_FOUND', 'Session not found.');
        return command.operation === 'env' ? selected.env : selected;
      }
      if (command.operation === 'delete' && !options.yes && !options.json && process.stdin.isTTY && process.stdout.isTTY) {
        const inspected = await managerControlRequest(host, options)(options.userData ?? '', { action: 'inspect', id: sessionId });
        process.stdout.write(formatControl('list', inspected, colorEnabled()) + formatControl('processes', inspected, colorEnabled()));
      }
      if (command.operation === 'delete' || (command.operation === 'settle' && command.payload.confirmActive === true)) await requireConfirmation(options, 'session ' + command.operation + ' ' + sessionId);
      const methods: Record<Exclude<typeof command.operation, 'inspect'>, ManagerMethod> = {
        rename: 'renameSession', pin: 'setSessionPinned', env: 'setSessionEnv', settle: 'settleSession', unsettle: 'unsettleSession',
        delete: 'deleteSession', focus: 'focusSession', refresh: 'refreshSessionMembership', 'clear-error': 'clearSessionError',
      };
      return request(host, methods[command.operation], { ...command.payload, sessionId }, options.host);
    }
    case 'keep-awake': {
      if (options.host && options.host !== 'local') throw new CliUsageError('Keep awake only manages the local host.');
      if (command.active === undefined) {
        if (!host.service.getKeepAwake) throw new CliFailure('UNSUPPORTED', 'Keep awake is unavailable.');
        return unwrap(await host.service.getKeepAwake());
      }
      return request(host, 'setKeepAwake', { active: command.active });
    }
    case 'request':
      if (command.destructive) await requireConfirmation(options, command.method);
      return request(host, command.method, command.payload, options.host);
    case 'create-session': {
      const { cwd } = unwrap(await service.resolveDirectory({ path: command.cwd }));
      return request(host, 'createSession', { cwd, requestId: command.requestId, ...(command.title === undefined ? {} : { title: command.title }) }, options.host);
    }
    case 'host':
      if (options.host && options.host !== 'local') throw new CliUsageError('Host lifecycle commands only operate on the local host.');
      if (command.operation === 'status') return host.status();
      if (command.operation === 'stop') { await requireConfirmation(options, 'Stop the host and all its terminals'); await host.stop(); return { stopped: true }; }
      throw new CliUsageError('host run must be invoked before connecting.');
    case 'view': {
      const snapshot = unwrap(await service.getSnapshot());
      if (command.view === 'settings') return snapshot.settings;
      const sessions = command.id ? snapshot.sessions.filter(session => session.id === command.id) : snapshot.sessions;
      if (command.id && !sessions.length) throw new CliFailure('NOT_FOUND', 'Session not found.');
      return sessions.flatMap(session => session.tabs);
    }
    case 'settings-update': {
      if (!command.patch || typeof command.patch !== 'object' || Array.isArray(command.patch)) throw new CliUsageError('Settings input must be a JSON object.');
      const settings = unwrap(await service.getSnapshot()).settings;
      return request(host, 'saveSettings', { ...settings, ...command.patch }, options.host);
    }
    case 'terminal': {
      if (command.operation === 'attach') {
        if (options.json) throw new CliUsageError('Interactive attach does not support --json. Use terminals write and api attachTerminal for automation.');
        if (!service.attachTerminal || !service.takeTerminalControl || !service.writeTerminal || !service.resizeTerminal || !service.acknowledgeTerminal || !service.detachTerminal || !service.subscribeTerminal) throw new CliFailure('UNSUPPORTED', 'This host does not support interactive attach.');
        const disconnected = (listener: () => void) => host.onDisconnect(listener);
        const io = attachIo ?? { input: process.stdin, output: process.stdout, signals: process, onDisconnect: disconnected };
        if (!io.input.isTTY || !io.output.isTTY) throw new CliUsageError('Interactive attach requires a TTY on stdin and stdout.');
        const api: AttachApi = { attachTerminal: service.attachTerminal, takeTerminalControl: service.takeTerminalControl, writeTerminal: service.writeTerminal, resizeTerminal: service.resizeTerminal,
          acknowledgeTerminal: service.acknowledgeTerminal, detachTerminal: service.detachTerminal, subscribeTerminal: service.subscribeTerminal };
        await attachTerminal(api, command.tabId, io);
        return undefined;
      }
      if (command.operation === 'close') await requireConfirmation(options, 'Close terminal ' + command.tabId);
      const snapshot = unwrap(await service.getSnapshot());
      const tab = snapshot.sessions.flatMap(session => session.tabs).find(tab => tab.id === command.tabId);
      if (!tab?.generation) throw new CliFailure('NOT_FOUND', 'Open embedded terminal not found.');
      const identity = { tabId: tab.id, generation: tab.generation };
      if (command.operation === 'close') return request(host, 'closeTab', identity, options.host);
      const data = command.stdin ? await readStdin(process.stdin, 65536) : command.data;
      if (!service.attachTerminal || !service.detachTerminal) throw new CliFailure('UNSUPPORTED', 'This host does not support exclusive terminal control.');
      const checked = requestSchemas.writeTerminal.safeParse({ ...identity, controlEpoch: 1, data });
      if (!checked.success) throw new CliUsageError('Terminal input must be nonempty and fit within 64 KiB.');
      // A one-shot write joins without stealing. --take is an explicit handoff.
      let attachment = unwrap(await service.attachTerminal(identity));
      try {
        if (attachment.control === 'blocked' && command.take) {
          await request(host, 'takeTerminalControl', identity, options.host);
          attachment = unwrap(await service.attachTerminal(identity));
        }
        if (attachment.control !== 'owned') throw new CliFailure('TERMINAL_CONTROL_REQUIRED', 'Another client is controlling this terminal. Use --take to switch control explicitly.');
        return await request(host, 'writeTerminal', { ...identity, controlEpoch: attachment.controlEpoch, data: checked.data.data }, options.host);
      } catch (error) {
        if (error instanceof CliFailure || error instanceof CliUsageError) throw error;
        throw new CliFailure('TARGET_LOST', 'Terminal input delivery is uncertain; input was not retried. Inspect the terminal before sending it again.');
      } finally { await service.detachTerminal(identity).catch(() => {}); }

    }
    case 'remote': return executeRemote(command, host, options);
    case 'remote-api': {
      if (options.host && options.host !== 'local') throw new CliUsageError('remote api manages the local host.');
      if (['forget', 'revoke', 'reject'].includes(command.method) || (command.method === 'configure' && !remoteRequestSchemas.configure.parse(command.payload).enabled)) await requireConfirmation(options, 'remote ' + command.method);
      if (command.method === 'request') {
        const input = remoteForwardRequestSchema.parse(command.payload);
        if (!isTerminalUploadMethod(input.method) && isDestructive(input.method, input.payload)) await requireConfirmation(options, input.method);
      }
      const value = await rawRequest(host, `remote.${command.method}`, command.payload);
      if (command.method === 'request') {
        const input = remoteForwardRequestSchema.parse(command.payload);
        const result = forwardedResponseSchemas[input.method].parse(value);
        if (!result.ok) throw new CliFailure(result.error.code, result.error.message, result.error.retryable);
        return result.value;
      }
      const result = hostResponseSchemas[`remote.${command.method}`].parse(value);
      if (!result || typeof result !== 'object' || !('ok' in result) || !result.ok || !('value' in result)) {
        if (result && typeof result === 'object' && 'error' in result) {
          const error = result.error;
          throw new CliFailure(error.code, error.message, error.retryable);
        }
        throw new CliFailure('INTERNAL', 'Invalid remote response.');
      }
      return result.value;
    }
  }
}
async function executeRemote(command: Extract<Command, { kind: 'remote' }>, host: CliHost, options: GlobalOptions): Promise<unknown> {
  if (options.host && options.host !== 'local') throw new CliUsageError('Network, machines, and approvals commands manage the local host. Use --host for session and terminal operations.');
  const remote = host.remote;
  if (['machines:forget', 'approvals:revoke', 'approvals:reject', 'network:disable'].includes(command.operation)) await requireConfirmation(options, command.operation);
  const peer = { peerId: command.peerId ?? '' };
  switch (command.operation) {
    case 'network:status': return unwrap(await remote.getState());
    case 'machines:list': return unwrap(await remote.getState()).peers.filter(peer => peer.hostConfirmed === true || (peer.hostConfirmed === undefined && peer.connection === 'connected'));
    case 'approvals:list': { const state = unwrap(await remote.getState()); return { approvals: state.approvals, pending: state.pending }; }
    case 'network:enable': case 'network:disable': {
      const state = unwrap(await remote.getState());
      return unwrap(await remote.configure({ enabled: command.operation === 'network:enable', hostname: command.hostname ?? state.hostname }));
    }
    case 'network:login': {
      const authKey = command.authKeyFile ? await readAuthKey(command.authKeyFile) : command.authKeyStdin ? validateAuthKey(await readStdin(process.stdin, 4096)) : undefined;
      try { return unwrap(await remote.login(authKey ? { authKey } : {})); }
      catch { throw new CliFailure('AUTH_FAILED', 'Network login failed. Check host network status.'); }
    }
    case 'machines:connect': return unwrap(await remote.connect(peer));
    case 'machines:disconnect': return unwrap(await remote.disconnect(peer));
    case 'machines:forget': return unwrap(await remote.forget(peer));
    case 'approvals:approve': return unwrap(await remote.approve(peer));
    case 'approvals:revoke': return unwrap(await remote.revoke(peer));
    case 'approvals:reject': return unwrap(await remote.reject(peer));
    default: throw new CliUsageError('Unknown remote operation.');
  }
}
