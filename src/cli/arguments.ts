import { adminOperandCount, takesOptionValue } from './global-values';
export class CliUsageError extends Error {
  readonly code = 'VALIDATION';
}
export interface GlobalOptions { userData?: string; host?: string; json: boolean; yes: boolean }
export interface Invocation { args: string[]; options: GlobalOptions }
/** Only script invocations are opaque. Administration and SSH accept trailing globals. */
export function parseGlobals(argv: string[]): Invocation {
  const options: GlobalOptions = { json: false, yes: false };
  const args: string[] = [], seen = new Set<string>();
  let commands: 'operation' | 'admin' | 'script' | undefined, operands = 0;
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--') { args.push(...argv.slice(i)); break; }
    if (operands > 0) { args.push(arg); operands--; continue; }
    if (commands === 'operation') {
      const count = adminOperandCount(arg);
      if (count !== undefined) { commands = 'admin'; operands = count; args.push(arg); continue; }
      if (arg === '--run') { commands = 'script'; args.push(arg); continue; }
      if (!arg.startsWith('-')) { args.push(...argv.slice(i)); break; }
    }
    if (commands === 'script' && !arg.startsWith('-')) { args.push(...argv.slice(i)); break; }
    const equal = arg.indexOf('=');
    const flag = equal < 0 ? arg : arg.slice(0, equal);
    const inline = equal < 0 ? undefined : arg.slice(equal + 1);
    if (!['--json', '--yes', '--host', '--user-data'].includes(flag)) {
      const namespace = args.length === 0;
      const value = inline === undefined && takesOptionValue(args, flag);
      args.push(arg);
      if (namespace && (arg === 'cmd' || arg === 'commands')) commands = 'operation';
      if (value && argv[i + 1] !== undefined && argv[i + 1] !== '--') args.push(argv[++i]);
      continue;
    }
    if (seen.has(flag)) throw new CliUsageError('Duplicate option: ' + flag);
    seen.add(flag);
    if (flag === '--json' || flag === '--yes') {
      if (inline !== undefined) throw new CliUsageError(flag + ' does not accept a value.');
      options[flag === '--json' ? 'json' : 'yes'] = true;
    } else {
      const value = inline ?? argv[++i];
      if (!value || value.startsWith('--') || /[\0\r\n]/.test(value)) throw new CliUsageError('Missing or invalid value for ' + flag);
      options[flag === '--host' ? 'host' : 'userData'] = value;
    }
  }
  return { args, options };
}
export class CommandArguments {
  readonly positionals: string[] = [];
  private readonly values = new Map<string, string | true>();
  constructor(args: string[], valueFlags: string[] = [], booleanFlags: string[] = []) {
    for (let i = 0; i < args.length; i++) {
      const arg = args[i];
      if (arg === '--') { this.positionals.push(...args.slice(i + 1)); break; }
      if (!arg.startsWith('--')) { this.positionals.push(arg); continue; }
      const equal = arg.indexOf('=');
      const flag = equal < 0 ? arg : arg.slice(0, equal);
      if (this.values.has(flag)) throw new CliUsageError('Duplicate option: ' + flag);
      if (booleanFlags.includes(flag)) {
        if (equal >= 0) throw new CliUsageError(flag + ' does not accept a value.');
        this.values.set(flag, true);
      } else if (valueFlags.includes(flag)) {
        const value = equal < 0 ? args[++i] : arg.slice(equal + 1);
        if (value === undefined || value === '--') throw new CliUsageError('Missing value for ' + flag);
        this.values.set(flag, value);
      } else throw new CliUsageError('Unknown option: ' + flag);
    }
  }
  text(flag: string): string | undefined { const value = this.values.get(flag); return typeof value === 'string' ? value : undefined; }
  has(flag: string): boolean { return this.values.has(flag); }
  arity(min: number, max = min): string[] {
    if (this.positionals.length < min || this.positionals.length > max) throw new CliUsageError('Expected ' + (min === max ? min : min + ' to ' + max) + ' positional arguments.');
    return this.positionals;
  }
}
export function parseJson(text: string): unknown {
  try { return JSON.parse(text); } catch { throw new CliUsageError('Invalid JSON input.'); }
}
