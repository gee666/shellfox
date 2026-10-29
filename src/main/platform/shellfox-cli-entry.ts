import path from 'node:path';
import { lstat } from 'node:fs/promises';
import { defaultUserData, runCli as runBuiltinCli } from '../ssh/cli';
import { dispatchCustomCli } from './shellfox-cli-dispatch';
import { dispatchDesktopLaunch } from './shellfox-cli-launch';
import { COMPLETION_HELP, completionScript, dispatchCompletion } from './shellfox-completion';
import { parseControlArgs, runControlCli } from '../control-cli';
import { connectHostClient, type HostClient } from '../../host/client';
import { parseGlobals, CliUsageError } from '../../cli/arguments';
import { parseCommand } from '../../cli/commands';
import { executeCommand } from '../../cli/execute';
import { parseAutomationCommand, runAutomationCommand } from '../../cli/automation';
import { CliFailure, printError, printValue, unwrap } from '../../cli/output';
import { colorEnabled } from '../../cli/human';
import { cliHelp, knownHelpTopic } from '../../cli/help';
import { resolveStartDirectory } from '../../cli/start';
import { runHostService } from '../../cli/service';
import { loadJsonArguments } from '../../cli/input';
import { applyRuntimeContext, resolveRuntimeContext } from '../../cli/runtime-context';
import { managerControlRequest } from '../../cli/control';
import { sessionNamespaces } from '../../cli/session';
import { trailingHelpRequested } from '../../cli/global-values';
import { parseUpdateArgs, runHeadlessUpdate } from '../../cli/update';
import { forwardHeadlessUpdate, watchUpdateParent } from '../../cli/update-activation';
declare const __SHELLFOX_VERSION__: string;
let foregroundHostSettled = false;
function decodeArguments(argv: string[]): string[] {
  if (argv[0] !== '--shellfox-argv-json') return argv;
  if (argv.length !== 2) throw new CliUsageError('Invalid encoded CLI arguments.');
  const decoded: unknown = JSON.parse(Buffer.from(argv[1], 'base64').toString('utf8'));
  if (!Array.isArray(decoded) || !decoded.every(arg => typeof arg === 'string' && !arg.includes('\0'))) throw new CliUsageError('Invalid encoded CLI arguments.');
  return decoded;
}
async function connect(userData: string): Promise<HostClient> {
  try { return await connectHostClient(userData); }
  catch { throw new CliFailure('HOST_UNAVAILABLE', 'Host connection failed. Start Shellfox or run shellfox host run with the same --user-data directory.'); }
}
async function resolveTarget(client: HostClient, target: string | undefined): Promise<string | undefined> {
  if (!target || target === 'local') return target;
  const state = unwrap(await client.remote.getState()), exact = state.peers.find(peer => peer.id === target);
  if (exact) return exact.id;
  const matches = state.peers.filter(peer => peer.hostname.toLowerCase() === target.toLowerCase());
  if (matches.length !== 1) throw new CliUsageError(matches.length ? 'Host name is ambiguous. Use a peer ID.' : 'Unknown host. Run shellfox machines list.');
  return matches[0].id;
}
export async function runShellfoxCli(argv: string[]): Promise<number> {
  let json = argv.includes('--json'), client: HostClient | undefined;
  let restoreContext = () => {};
  try {
    argv = decodeArguments(argv); json = argv.includes('--json');
    const forwarded = await forwardHeadlessUpdate(argv, __filename);
    if (forwarded !== null) return forwarded;
    let defaultData: string | undefined;
    if (argv[0] === '--shellfox-default-user-data') {
      if (!argv[1] || /[\0\r\n]/.test(argv[1])) throw new CliUsageError('Invalid launcher user data directory.');
      defaultData = argv[1]; argv = argv.slice(2);
    }
    const { args, options } = parseGlobals(argv); json = options.json;
    const userData = path.resolve(options.userData ?? defaultData ?? process.env.SHELLFOX_USER_DATA ?? defaultUserData());
    options.userData = userData;
    const rootHelp = !args.length || (args.length === 1 && ['help', '--help', '-h'].includes(args[0]));
    const helpTopic = args[0] === 'help' ? args[1] : args.length === 1 && ['commands', 'cmd'].includes(args[0]) ? args[0] : knownHelpTopic(args[0]) && args[0] !== 'ssh' && trailingHelpRequested(args) ? args[0] : undefined;
    if (rootHelp || helpTopic) {
      if (helpTopic && !knownHelpTopic(helpTopic)) throw new CliUsageError('Unknown help group. Run shellfox help.');
      const help = cliHelp(__SHELLFOX_VERSION__, rootHelp ? undefined : helpTopic, !json && colorEnabled());
      if (json) printValue(help, true, process.stdout); else process.stdout.write(help);
      return 0;
    }
    if (['version', '--version', '-v'].includes(args[0]) && args.length === 1) { printValue(__SHELLFOX_VERSION__, json, process.stdout); return 0; }
    if (args[0] === 'completion' && json) {
      const script = args.length === 2 ? completionScript(args[1]) : null;
      if (!script) throw new CliUsageError(COMPLETION_HELP.trim());
      printValue(script, true, process.stdout); return 0;
    }
    const completion = dispatchCompletion(args); if (completion !== null) return completion;
    if (args[0] === 'commands' || args[0] === 'cmd') {
      if (options.host && options.host !== 'local') throw new CliUsageError('Custom commands run locally, not through --host.');
      const commandArgs = ['cmd', ...args.slice(1)];
      if (!json) return (await dispatchCustomCli(commandArgs, userData)) ?? 2;
      if (!['--register', '--ls', '--list', '--remove', '--rm', '--unregister', '--rename', '--complete', '--help', '-h'].includes(args[1] ?? '--help')) throw new CliUsageError('Custom script execution does not support --json.');
      let output = '', error = '';
      const code = await dispatchCustomCli(commandArgs, userData, { output: { write: text => { output += text; } }, error: { write: text => { error += text; } } });
      if (code !== 0) throw code === 2 ? new CliUsageError(error.trim()) : new CliFailure('COMMAND_FAILED', error.trim());
      printValue(output.trimEnd(), true, process.stdout); return 0;
    }
    if (args[0] === 'ssh') {
      if (json || (options.host && options.host !== 'local')) throw new CliUsageError('SSH is interactive and local. It does not support --json or a remote --host.');
      return runBuiltinCli(['--user-data', userData, ...args]);
    }
    const runtimeContext = await resolveRuntimeContext(__filename);
    restoreContext = applyRuntimeContext(runtimeContext);
    if (args[0] === 'update') {
      const update = parseUpdateArgs(args.slice(1), options.host);
      if (runtimeContext.headless) {
        const result = await runHeadlessUpdate({ entry: __filename, current: __SHELLFOX_VERSION__, ...update });
        printValue(json ? result : result.message, json, process.stdout); return 0;
      }
      if (json) throw new CliUsageError('Desktop updates use api getUpdateStatus or api downloadUpdate for JSON automation.');
      const code = await dispatchDesktopLaunch(args); if (code !== null) return code;
      throw new CliFailure('UNSUPPORTED', 'This desktop CLI cannot install updates directly. Use the installed desktop launcher or the in-app updater. Headless deployments need the separate headless release package.');
    }
    if (args[0] === 'host' && args[1] === 'service') {
      const result = await runHostService(args.slice(2), userData, options);
      if (json) printValue(result, true, process.stdout); return 0;
    }
    const automation = parseAutomationCommand(args);
    if (automation) {
      if (options.host && options.host !== 'local') throw new CliUsageError('Automation commands use the local host. Run the CLI on the target machine.');
      client = await connect(userData);
      return await runAutomationCommand(automation, client, options);
    }
    const jsonInput = args[0] === 'api' || (args[0] === 'remote' && args[1] === 'api') || (args[0] === 'settings' && args[1] === 'update') || (sessionNamespaces.includes(args[0]) && args[1] === 'env');
    const command = parseCommand(jsonInput ? await loadJsonArguments(args) : args, Boolean(options.host && options.host !== 'local'));
    if (command?.kind === 'host' && command.operation === 'run') {
      if (options.host && options.host !== 'local') throw new CliUsageError('host run only starts the local host.');
      const { runHost } = await import('../../host/runtime');
      try {
        await runHost({ userData, resourcesPath: process.env.SHELLFOX_RESOURCES_PATH, projectRoot: process.env.SHELLFOX_PROJECT_ROOT,
          cliBinding: { executable: process.execPath, entry: __filename, runAsNode: Boolean(process.versions.electron) }, onReady: status => {
            if (!json) process.stdout.write('Host running. Press Ctrl+C to stop.\n'); printValue(status, json, process.stdout);
          } });
      } finally { foregroundHostSettled = true; }
      return 0;
    }
    if (command?.kind === 'control') {
      const parsed = parseControlArgs(command.args);
      if (options.host && options.host !== 'local') {
        if (parsed.kind === 'control' && parsed.filters.current) throw new CliUsageError('A remote session ID or --all is required; local terminal context is never forwarded.');
      }
      client = await connect(userData); options.host = await resolveTarget(client, options.host);
      return await runControlCli(command.args, userData, __SHELLFOX_VERSION__, { json, yes: options.yes, color: colorEnabled(), request: managerControlRequest(client, options) });
    }
    if (command?.kind === 'terminal' && command.operation === 'attach') {
      if (json) throw new CliUsageError('Interactive attach does not support --json.');
      if (!process.stdin.isTTY || !process.stdout.isTTY) throw new CliUsageError('Interactive attach requires a TTY on stdin and stdout.');
    }
    if (command?.kind === 'create-session' && (!options.host || options.host === 'local')) command.cwd = await resolveStartDirectory(command.cwd);
    if (command) {
      try { client = await connect(userData); }
      catch (error) {
        if (command.kind !== 'create-session' || process.env.SHELLFOX_HEADLESS === '1' || json || (options.host && options.host !== 'local')) throw error;
        try { await lstat(path.join(userData, 'host', 'endpoint.json')); throw error; }
        catch (missing) { if (!(missing instanceof Error) || !('code' in missing) || missing.code !== 'ENOENT') throw missing; }
        const code = await dispatchDesktopLaunch(['start', command.cwd]); if (code !== null) return code;
        throw error;
      }
      options.host = await resolveTarget(client, options.host);
      const result = await executeCommand(command, client, options);
      if (!(command.kind === 'terminal' && command.operation === 'attach')) printValue(result, json, process.stdout);
      return 0;
    }
    throw new CliUsageError('Unknown command. Run shellfox help. Session operations belong under shellfox session; terminals use shellfox terminals.');
  } catch (error) { return printError(error, json, process.stdout, process.stderr); }
  finally { try { await client?.close(); } finally { restoreContext(); } }
}
if (require.main === module) {
  watchUpdateParent();
  void runShellfoxCli(process.argv.slice(2)).then(async code => {
  process.exitCode = code;
  if (foregroundHostSettled) {
    await Promise.all([process.stdout, process.stderr].map(stream => new Promise<void>(resolve => {
      if (stream.destroyed || stream.writableEnded) resolve(); else stream.write('', () => resolve());
    })));
    process.exit(code);
  }
}).catch(error => { process.stderr.write('Shellfox: ' + (error instanceof Error ? error.message : 'Command failed.') + '\n'); process.exitCode = 1; });
}
