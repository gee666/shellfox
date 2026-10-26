const commandOperands: Record<string, number> = {
  '--register': 2, '--remove': 1, '--rm': 1, '--unregister': 1, '--rename': 2, '--complete': 1,
  '--ls': 0, '--list': 0, '--help': 0, '-h': 0,
};
export function adminOperandCount(operation: string): number | undefined { return Object.hasOwn(commandOperands, operation) ? commandOperands[operation] : undefined; }
export function trailingHelpRequested(args: string[]): boolean {
  const commands = args[0] === 'commands' || args[0] === 'cmd';
  let operands = 0;
  for (let i = 1; i < args.length; i++) {
    const arg = args[i];
    if (arg === '--') return false;
    if (operands > 0) { operands--; continue; }
    const help = arg === '--help' || arg === '-h' || (arg === 'help' && args.length === 2 && !commands);
    if (i === args.length - 1 && help) return true;
    if (commands && i === 1) {
      const count = adminOperandCount(arg);
      if (count === undefined) return false;
      operands = count;
    } else if (!arg.includes('=') && takesOptionValue(args, arg)) operands = 1;
  }
  return false;
}
/** Protect required option values before looking for global flags in the remaining argv. */
export function takesOptionValue(args: string[], flag: string): boolean {
  const [namespace, operation] = args;
  if (namespace === 'ssh') return flag === '--profile' || flag === '-p';
  if (['session', 'ss', 'sessions'].includes(namespace)) {
    if (operation === 'create') return ['--title', '--request-id'].includes(flag);
    if (['list', 'stats', 'processes'].includes(operation)) return ['--id', '--name', '--path', '--tab', '--pid'].includes(flag);
    return flag === '--id' || (operation === 'env' && flag === '--input');
  }
  if (namespace === 'terminals' || namespace === 'tm') return flag === '--id' || (operation === 'create' && ['--title', '--profile', '--cwd'].includes(flag));
  if (namespace === 'api' || (namespace === 'remote' && operation === 'api') || (namespace === 'settings' && operation === 'update')) return flag === '--input';
  if (namespace === 'network') return (operation === 'enable' && flag === '--hostname') || (operation === 'login' && flag === '--auth-key-file');
  return false;
}
