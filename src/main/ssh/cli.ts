import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { homedir } from 'node:os';
import { SshProfileStore } from './storage';
import { CliTerminal, Cancelled } from './terminal';
import { connectSsh, SshConnectionError } from './client';
import { helpText, sshHelp, parseSshArgs, styles } from './presentation';
declare const __SHELLFOX_VERSION__: string;
export function defaultUserData(platform = process.platform, env = process.env, home = homedir()): string {
  return platform === 'win32' ? path.win32.join(env.APPDATA || path.win32.join(home,'AppData','Roaming'),'Shellfox') : platform === 'darwin' ? path.join(home,'Library','Application Support','Shellfox') : path.join(env.XDG_CONFIG_HOME || path.join(home,'.config'),'Shellfox');
}
export async function runCli(argv: string[]): Promise<number> {
  const terminal = new CliTerminal(), s = styles(terminal.color);
  let userData=defaultUserData();
  if(argv[0]==='--user-data'){if(!argv[1])throw new Error('missing user data directory');userData=path.resolve(argv[1]);argv=argv.slice(2);}
  if(argv.length===1&&argv[0]==='--help'){process.stdout.write(helpText(__SHELLFOX_VERSION__,terminal.color,process.platform!=='win32'));return 0;}
  if(argv[0]!=='ssh'){process.stdout.write('  '+s.red('invalid command')+'\n  '+s.dim('run shellfox --help')+'\n');return 2;}
  let args: ReturnType<typeof parseSshArgs>;
  try{args=parseSshArgs(argv.slice(1));}catch{process.stdout.write('  '+s.red('invalid ssh arguments')+'\n  '+s.dim('run shellfox --help')+'\n');return 2;}
  if(args.help){process.stdout.write(sshHelp(terminal.color));return 0;}
  try {
    const profiles=(await new SshProfileStore(userData).read()).sort((a,b)=>a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
    if(!profiles.length){process.stdout.write('  no ssh connections yet\n  add them in Shellfox › Settings › SSH connections  (or import from PuTTY there)\n');return args.profile?1:terminal.tty?0:2;}
    let selected=args.profile?profiles.find(p=>p.name.toLowerCase()===args.profile!.toLowerCase()):undefined;
    if(args.profile&&!selected){process.stdout.write('  '+s.red('no connection named '+JSON.stringify(args.profile))+'\n');profiles.forEach(p=>process.stdout.write('  '+s.dim(p.name)+'\n'));return 1;}
    if(!terminal.tty&&!selected){profiles.forEach(p=>process.stdout.write('  '+p.name+'\n'));process.stdout.write(sshHelp(false));return 2;}
    terminal.start();selected??=await terminal.pick(profiles);
    if(process.env.WSL_DISTRO_NAME && selected.keyFile && /^[A-Za-z]:[\\\\/]|^\\\\\\\\/.test(selected.keyFile)) {
      selected={...selected,keyFile:execFileSync('wslpath',['-u',selected.keyFile],{encoding:'utf8',timeout:5000}).trim()};
    }
    const code=await connectSsh(selected,userData,terminal);
    terminal.restore();process.stdout.write('\n  '+s.dim(`connection to ${selected.name} closed${code?' (exit '+code+')':''}`)+'\n');return code;
  }catch(e){terminal.restore();if(e instanceof Cancelled)return 130;process.stdout.write('  '+s.red((e as Error).message.replace(/[\x00-\x1f\x7f]/g,' '))+'\n');return e instanceof SshConnectionError?255:1;}
  finally{terminal.restore();}
}
if(require.main===module)void runCli(process.argv.slice(2)).then(code=>{process.exitCode=code;}).catch(e=>{console.error('  '+(e as Error).message);process.exitCode=1;});
