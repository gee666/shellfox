import path from 'node:path';
import { defaultUserData, runCli as runBuiltinCli } from '../ssh/cli';
import { CUSTOM_COMMAND_HELP, dispatchCustomCli } from './shellfox-cli-dispatch';
import { dispatchWindowsLaunch } from './shellfox-cli-launch';
import { COMPLETION_HELP, dispatchCompletion } from './shellfox-completion';

export async function runShellfoxCli(argv: string[]): Promise<number> {
  if (argv[0] === '--shellfox-argv-json') {
    if (argv.length !== 2) throw new Error('Invalid encoded CLI arguments.');
    const decoded: unknown = JSON.parse(Buffer.from(argv[1], 'base64').toString('utf8'));
    if (!Array.isArray(decoded) || !decoded.every(arg => typeof arg === 'string' && !arg.includes('\0'))) throw new Error('Invalid encoded CLI arguments.');
    argv = decoded;
  }
  let userData = defaultUserData();
  let args = argv;
  if (argv[0] === '--user-data') {
    if (!argv[1]) throw new Error('Missing user data directory.');
    userData = path.resolve(argv[1]); args = argv.slice(2);
  }
  const completion = dispatchCompletion(args);
  if (completion !== null) return completion;
  const custom = await dispatchCustomCli(args, userData);
  if (custom !== null) return custom;
  const launch = await dispatchWindowsLaunch(args);
  if (launch !== null) return launch;
  const code = await runBuiltinCli(argv);
  if (code === 0 && (!args.length || (args.length === 1 && ['help', '--help', '-h'].includes(args[0])))) process.stdout.write('\n' + CUSTOM_COMMAND_HELP + '\n' + COMPLETION_HELP);
  return code;
}
if (require.main === module) void runShellfoxCli(process.argv.slice(2)).then(code => { process.exitCode = code; }).catch(error => {
  console.error('Shellfox: ' + JSON.stringify((error as Error).message)); process.exitCode = 1;
});
