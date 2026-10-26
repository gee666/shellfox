import { randomUUID } from 'node:crypto';
import { idSchema, requestSchemas, type ManagerMethod } from '../shared/schemas';
import { CommandArguments, CliUsageError, parseJson } from './arguments';
import { explicitSessionId, parseSessionAction, sessionNamespaces, type SessionCommand } from './session';
import { parseControlArgs } from '../main/control-cli';
import { remoteEnvelopeSchema, remoteRequestSchemas, remotePayloadSchemas, remotePeerIdSchema, remoteHostnameSchema } from '../shared/remote-schemas';
export type Command =
  | { kind: 'request'; method: ManagerMethod; payload: unknown; destructive: boolean }
  | { kind: 'view'; view: 'terminals' | 'settings'; id?: string }
  | { kind: 'settings-update'; patch: unknown }
  | { kind: 'create-session'; cwd: string; requestId: string; title?: string }
  | { kind: 'terminal'; operation: 'attach' | 'write' | 'close'; tabId: string; data?: string; stdin?: boolean }
  | { kind: 'terminal-session'; operation: 'create' | 'rename' | 'list' | 'reorder'; sessionId?: string; payload: Record<string, unknown> }
  | { kind: 'host'; operation: 'run' | 'status' | 'stop' }
  | { kind: 'remote'; operation: string; peerId?: string; hostname?: string; authKeyStdin?: boolean; authKeyFile?: string }
  | { kind: 'remote-api'; method: keyof typeof remoteRequestSchemas; payload: unknown }
  | { kind: 'control'; args: string[] }
  | { kind: 'keep-awake'; active?: boolean }
  | SessionCommand;
export function isDestructive(method: ManagerMethod, payload: unknown): boolean {
  return ['deleteSession', 'closeTab', 'deleteSshProfile', 'installUpdate', 'importPuttySessions', 'retryTab'].includes(method) ||
    (method === 'settleSession' && requestSchemas.settleSession.parse(payload).confirmActive);
}
function request(method: ManagerMethod, payload: unknown = {}, remote = false): Command {
  const parsed = (remote ? remotePayloadSchemas : requestSchemas)[method].safeParse(payload);
  if (!parsed.success) throw new CliUsageError('Invalid ' + method + ' input: ' + parsed.error.issues.map(issue => issue.path.join('.') + ' ' + issue.message).join('; '));
  return { kind: 'request', method, payload: parsed.data,
    destructive: isDestructive(method, parsed.data) };
}
function jsonInput(args: CommandArguments, fallback?: unknown): unknown {
  const input = args.text('--input');
  if (input === undefined && fallback === undefined) throw new CliUsageError('Provide --input with JSON.');
  return input === undefined ? fallback : parseJson(input);
}
export function parseCommand(argv: string[], remote = false): Command | null {
  const [namespace, operation, ...rest] = argv;
  if (namespace === 'remote' && operation === 'api') {
    const [method, ...tail] = rest;
    const args = new CommandArguments(tail, ['--input']); args.arity(0);
    const envelope = remoteEnvelopeSchema.safeParse({ method, payload: jsonInput(args, {}) });
    if (!envelope.success) throw new CliUsageError('Unknown remote method.');
    const parsed = remoteRequestSchemas[envelope.data.method].safeParse(envelope.data.payload);
    if (!parsed.success) throw new CliUsageError('Invalid remote input.');
    if (envelope.data.method === 'login' && 'authKey' in parsed.data) throw new CliUsageError('Use network login --auth-key-stdin or --auth-key-file. Auth keys must not appear in argv.');
    return { kind: 'remote-api', method: envelope.data.method, payload: parsed.data };
  }
  if (namespace === 'api') {
    if (!operation || !Object.hasOwn(requestSchemas, operation)) throw new CliUsageError('Unknown ManagerMethod.');
    const args = new CommandArguments(rest, ['--input']); args.arity(0);
    // Object.hasOwn does not narrow string keys in TypeScript.
    const method = Object.keys(requestSchemas).find((key): key is ManagerMethod => key === operation);
    if (!method) throw new CliUsageError('Unknown ManagerMethod.');
    return request(method, jsonInput(args, {}), remote);
  }
  if (namespace === 'host') {
    if (operation === 'keep-awake') {
      const args = new CommandArguments(rest); const [state] = args.arity(0, 1);
      if (state && !['on', 'off'].includes(state)) throw new CliUsageError('Usage: shellfox host keep-awake [on|off]');
      return { kind: 'keep-awake', active: state === undefined ? undefined : state === 'on' };
    }
    const args = new CommandArguments(rest); args.arity(0);
    if (!['run', 'status', 'stop'].includes(operation)) throw new CliUsageError('Usage: shellfox host run|status|stop');
    if (operation === 'run') return { kind: 'host', operation: 'run' };
    return { kind: 'host', operation: operation === 'status' ? 'status' : 'stop' };
  }
  if (['network', 'machines', 'approvals'].includes(namespace)) {
    const args = new CommandArguments(rest, ['--hostname', '--auth-key-file'], ['--auth-key-stdin']);
    const operations = namespace === 'network' ? ['enable', 'disable', 'login', 'status'] : namespace === 'machines' ? ['list', 'connect', 'disconnect', 'forget'] : ['list', 'approve', 'revoke', 'reject'];
    if (!operations.includes(operation)) throw new CliUsageError('Unknown ' + namespace + ' operation.');
    const peer = namespace !== 'network' && operation !== 'list'; args.arity(peer ? 1 : 0);
    if (peer && !remotePeerIdSchema.safeParse(args.positionals[0]).success) throw new CliUsageError('Invalid peer ID.');
    if ((args.has('--auth-key-stdin') || args.has('--auth-key-file')) && (namespace !== 'network' || operation !== 'login')) throw new CliUsageError('Auth keys are only accepted by network login.');
    if (args.has('--auth-key-file') && args.has('--auth-key-stdin')) throw new CliUsageError('Choose --auth-key-stdin or --auth-key-file.');
    if (args.has('--hostname') && (namespace !== 'network' || operation !== 'enable')) throw new CliUsageError('--hostname is only accepted by network enable.');
    if (args.has('--hostname') && !remoteHostnameSchema.safeParse(args.text('--hostname')).success) throw new CliUsageError('Invalid network hostname.');
    return { kind: 'remote', operation: namespace + ':' + operation, peerId: args.positionals[0], hostname: args.text('--hostname'), authKeyStdin: args.has('--auth-key-stdin'), authKeyFile: args.text('--auth-key-file') };
  }
  if (sessionNamespaces.includes(namespace)) {
    if (['list', 'stats', 'processes'].includes(operation)) {
      parseControlArgs(['session', operation, ...rest]);
      return { kind: 'control', args: ['session', operation, ...rest] };
    }
    if (operation === 'create') {
      const args = new CommandArguments(rest, ['--title', '--request-id']);
      const [cwd] = args.arity(0, 1);
      const input = { requestId: args.text('--request-id') ?? randomUUID(), title: args.text('--title') };
      if (!requestSchemas.createSession.omit({ cwd: true }).safeParse(input).success) throw new CliUsageError('Invalid session title or request ID.');
      return { kind: 'create-session', cwd: remote ? cwd ?? '~' : cwd ?? '.', ...input };
    }
    return parseSessionAction(operation, rest);
  }
  if (namespace === 'terminals' || namespace === 'tm') {
    const args = new CommandArguments(rest, operation === 'create' ? ['--title', '--profile', '--cwd', '--id'] : ['rename', 'list', 'reorder'].includes(operation) ? ['--id'] : [], operation === 'write' ? ['--stdin'] : ['create', 'rename', 'list', 'reorder'].includes(operation) ? ['--current'] : []);
    const [id, value, ...remaining] = args.positionals;
    if (id !== undefined && !idSchema.safeParse(id).success) throw new CliUsageError('Expected a full session or terminal UUID.');
    switch (operation) {
      case 'list': {
        args.arity(0, 1); const sessionId = explicitSessionId(id, args.text('--id'));
        return args.has('--current') && !sessionId ? { kind: 'terminal-session', operation: 'list', payload: {} } : { kind: 'view', view: 'terminals', id: sessionId };
      }
      case 'create': args.arity(0, 1); return { kind: 'terminal-session', operation: 'create', sessionId: explicitSessionId(id, args.text('--id')), payload: { ...(args.text('--title') === undefined ? {} : { title: args.text('--title') }), ...(args.text('--profile') ? { profileId: args.text('--profile') } : {}), ...(args.text('--cwd') ? { cwd: args.text('--cwd') } : {}) } };
      case 'rename': {
        args.arity(2, 3);
        const hasSession = args.positionals.length === 3;
        return { kind: 'terminal-session', operation: 'rename', sessionId: explicitSessionId(hasSession ? id : undefined, args.text('--id')), payload: { tabId: hasSession ? value : id, title: hasSession ? remaining[0] : value } };
      }
      case 'reorder': {
        const contextual = args.has('--current') || args.has('--id') || args.positionals.length === 1;
        args.arity(contextual ? 1 : 2, 1001);
        return { kind: 'terminal-session', operation: 'reorder', sessionId: contextual ? explicitSessionId(undefined, args.text('--id')) : id,
          payload: { tabIds: contextual ? args.positionals : [value, ...remaining] } };
      }
      case 'attach': case 'close': args.arity(1); return { kind: 'terminal', operation, tabId: id };
      case 'write':
        args.arity(args.has('--stdin') ? 1 : 2);
        return { kind: 'terminal', operation: 'write', tabId: id, data: value, stdin: args.has('--stdin') };
      default: throw new CliUsageError('Unknown terminals operation.');
    }
  }
  if (namespace === 'settings') {
    const args = new CommandArguments(rest, operation === 'update' ? ['--input'] : []); args.arity(0);
    if (operation === 'read') return { kind: 'view', view: 'settings' };
    if (operation === 'update') return { kind: 'settings-update', patch: jsonInput(args) };
    throw new CliUsageError('Usage: shellfox settings read|update --input <JSON>');
  }
  if (namespace === 'profiles') {
    new CommandArguments(argv.slice(2)).arity(0);
    if (operation && operation !== 'list') throw new CliUsageError('Usage: shellfox profiles [list]');
    return request('getTerminalProfiles');
  }
  if (namespace === 'path') {
    const args = new CommandArguments(rest);
    if (operation === 'home') { args.arity(0); return request('getHomeDirectory'); }
    args.arity(1);
    if (operation === 'create') return request('createDirectory', { path: args.positionals[0] });
    if (operation === 'resolve') return request('resolveDirectory', { path: args.positionals[0] });
    if (operation === 'complete') return request('completeDirectory', { path: args.positionals[0] });
    throw new CliUsageError('Usage: shellfox path home|resolve|complete [path]');
  }
  return null;
}
