import { colorEnabled } from '../../cli/human';
import { styles, terminalWidth } from '../../cli/terminal-text';
import { formatTable } from '../../cli/table';
import { CustomCommandStore, commandKey, validateCommandName } from './shellfox-cli-commands';
import { executeScript, resolveScriptLocation, scriptContext, type ScriptContext } from './shellfox-cli-scripts';

export const CUSTOM_COMMAND_HELP = `Custom commands:
  shellfox cmd --register <name> <script-path>
  shellfox cmd --ls | --list
  shellfox cmd --remove <name> | --rm <name> | --unregister <name>
  shellfox cmd --rename <old-name> <new-name>
  shellfox cmd <name> [args...]
  shellfox cmd --run <name> [args...]
  shellfox cmd --help
Use shellfox completion <powershell|bash|zsh> for Tab completion.
Paths are references. Each invocation reads the current script.
All arguments after the name go to the script unchanged.
`;
export interface CustomCliOptions {
  output?: { write(text: string): unknown; isTTY?: boolean; columns?: number };
  error?: { write(text: string): unknown };
  context?: ScriptContext;
  execute?: typeof executeScript;
  resolve?: typeof resolveScriptLocation;
}
const display = (text: string) => JSON.stringify(text).replace(/[\u007f-\u009f]/g, char => '\\u' + char.charCodeAt(0).toString(16).padStart(4, '0'));
class UsageError extends Error {}
/** Only the cmd namespace is handled here. Other commands never read the registry. */
export async function dispatchCustomCli(argv: string[], userData: string, options: CustomCliOptions = {}): Promise<number | null> {
  if (commandKey(argv[0] ?? '') !== 'cmd') return null;
  const destination = options.output ?? process.stdout, error = options.error ?? process.stderr;
  const color = argv[1] !== '--complete' && colorEnabled(destination);
  const output = { write: (text: string) => destination.write(color ? styles(true).accent(text) : text) };
  const store = new CustomCommandStore(userData);
  const args = argv.slice(1), operation = args.shift();
  try {
    if (operation === undefined || ['--help', '-h'].includes(operation)) {
      if (args.length) throw new UsageError('Usage: shellfox cmd --help');
      output.write(CUSTOM_COMMAND_HELP); return 0;
    }
    if (operation === '--complete') {
      if (args.length !== 1) throw new UsageError('Usage: shellfox cmd --complete <prefix>');
      const prefix = commandKey(args[0]);
      const names = (await store.read()).map(command => command.name)
        .filter(name => commandKey(name).startsWith(prefix))
        .sort((a, b) => commandKey(a).localeCompare(commandKey(b)));
      if (names.length) output.write(names.join('\n') + '\n');
      return 0;
    }
    if (operation === '--register') {
      if (args.length !== 2) throw new UsageError('Usage: shellfox cmd --register <name> <script-path>');
      validateCommandName(args[0]);
      await store.register(args[0], await (options.resolve ?? resolveScriptLocation)(args[1], options.context ?? scriptContext()));
      output.write('Registered ' + display(args[0]) + '.\n'); return 0;
    }
    if (['--remove', '--rm', '--unregister'].includes(operation)) {
      if (args.length !== 1) throw new UsageError('Usage: shellfox cmd ' + operation + ' <name>');
      await store.unregister(args[0]); output.write('Removed ' + display(args[0]) + '.\n'); return 0;
    }
    if (operation === '--rename') {
      if (args.length !== 2) throw new UsageError('Usage: shellfox cmd --rename <old-name> <new-name>');
      await store.rename(args[0], args[1]); output.write('Renamed ' + display(args[0]) + ' to ' + display(args[1]) + '.\n'); return 0;
    }
    if (['--ls', '--list'].includes(operation)) {
      if (args.length) throw new UsageError('Usage: shellfox cmd ' + operation);
      const commands = (await store.read()).sort((a, b) => commandKey(a.name).localeCompare(commandKey(b.name)));
      if (commands.length) {
        const wsl = commands.some(custom => custom.distro);
        output.write(formatTable(wsl ? ['NAME', 'PATH', 'WSL'] : ['NAME', 'PATH'], commands.map(custom =>
          [custom.name, display(custom.path), ...(wsl ? [custom.distro ? display(custom.distro) : 'none'] : [])]), false, terminalWidth(destination)) + '\n');
      } else output.write('No custom commands.\n');
      return 0;
    }
    const name = operation === '--run' ? args.shift() : operation;
    if (!name || name.startsWith('-')) throw new UsageError('Usage: shellfox cmd [--run] <name> [args...]');
    validateCommandName(name);
    const custom = (await store.read()).find(item => commandKey(item.name) === commandKey(name));
    if (!custom) throw new Error('No custom command named ' + name);
    return await (options.execute ?? executeScript)(custom, args, options.context ?? scriptContext());
  } catch (cause) { error.write('Shellfox: ' + display((cause as Error).message) + '\n'); return cause instanceof UsageError ? 2 : 1; }
}
