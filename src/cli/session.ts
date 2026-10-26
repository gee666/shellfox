import { CommandArguments, CliUsageError, parseJson } from './arguments';
import { idSchema, requestSchemas } from '../shared/schemas';
export const sessionNamespaces = ['session', 'ss', 'sessions'];
export interface SessionCommand {
  kind: 'session'; operation: 'inspect' | 'rename' | 'pin' | 'env' | 'settle' | 'unsettle' | 'delete' | 'focus' | 'refresh' | 'clear-error';
  sessionId?: string; payload: Record<string, unknown>;
}
export function explicitSessionId(positional: string | undefined, flag: string | undefined): string | undefined {
  if (positional && flag) throw new CliUsageError('Choose one explicit session ID, not both a positional ID and --id.');
  const id = positional ?? flag;
  if (id !== undefined && !idSchema.safeParse(id).success) throw new CliUsageError('Expected a full session UUID.');
  return id?.toLowerCase();
}
export function parseSessionAction(operation: string, rest: string[]): SessionCommand {
  if (operation === 'rm' || operation === '-rm') operation = 'delete';
  const values = operation === 'env' ? ['--id', '--input'] : ['--id'];
  const booleans = ['--current', ...(operation === 'pin' ? ['--off'] : []), ...(operation === 'settle' ? ['--confirm-active'] : [])];
  const args = new CommandArguments(rest, values, booleans);
  const payload: Record<string, unknown> = {};
  let id: string | undefined;
  if (operation === 'rename') {
    const positional = args.arity(1, 2);
    if (positional.length === 1 && !args.has('--id') && idSchema.safeParse(positional[0]).success) throw new CliUsageError('Missing the new title after the session UUID.');
    id = explicitSessionId(positional.length === 2 ? positional[0] : undefined, args.text('--id'));
    payload.title = positional.at(-1);
    if (!requestSchemas.renameSession.shape.title.safeParse(payload.title).success) throw new CliUsageError('Invalid session title.');
  } else {
    const [positional] = args.arity(0, 1);
    id = explicitSessionId(positional, args.text('--id'));
  }
  switch (operation) {
    case 'inspect': case 'unsettle': case 'delete': case 'focus': case 'refresh': case 'clear-error': break;
    case 'rename': break;
    case 'pin': payload.pinned = !args.has('--off'); break;
    case 'env': if (args.has('--input')) payload.env = parseJson(args.text('--input') ?? ''); break;
    case 'settle': payload.confirmActive = args.has('--confirm-active'); break;
    default: throw new CliUsageError('Unknown session operation. Run shellfox session --help.');
  }
  return { kind: 'session', operation, sessionId: id, payload };
}
