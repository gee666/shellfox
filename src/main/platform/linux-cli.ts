import path from 'node:path';
import { homedir } from 'node:os';
import { access, readFile, writeFile, mkdir, chmod, unlink, lstat } from 'node:fs/promises';
import { constants } from 'node:fs';
import type { CliIntegrationDto, Result } from '../../shared/contracts';
import { failure, success } from '../../shared/contracts';
import type { CliPort } from './shellfox-cli';
import updateScript from '../update/shellfox-update.sh';
export const LINUX_OWNER = 'Shellfox/linux-v1';
export const ownsLinuxFile = (content: string) => content.split(/\r?\n/).includes('# ' + LINUX_OWNER);
export interface LinuxCliOptions { executable: string; appPath?: string; home?: string; env?: NodeJS.ProcessEnv; prefixArgs?: string[]; launchEnv?: Record<string,string>; systemLauncher?: string }
export const shQuote = (value: string) => "'"+value.replaceAll("'","'\\''")+"'";
export function linuxLauncher(options: LinuxCliOptions): string {
  const target=[options.executable,...(options.appPath?[options.appPath]:[]),...(options.prefixArgs??[])].map(shQuote).join(' ');
  return `#!/bin/sh
# ${LINUX_OWNER}
if [ "\${1:-}" = '--help' ]; then printf '%s\\n' 'Usage: shellfox start [path]' '       shellfox update [--check]' 'Without arguments, open Shellfox.' 'update installs the latest published release.'; exit 0; fi
if [ "\${1:-}" = update ]; then
  shift
  shellfox_update_script=${shQuote(updateScript)}
  SHELLFOX_APP_EXECUTABLE=${shQuote(options.executable)} exec /bin/sh -c "$shellfox_update_script" shellfox-update "$@"
fi
if [ "$#" -eq 0 ]; then mode=show
elif [ "$1" = start ] && [ "$#" -le 2 ]; then
  target=\${2:-.}
  case "$target" in /*) ;; *) target=./$target ;; esac
  folder=$(CDPATH='' cd -P -- "$target" 2>/dev/null && pwd -P) || { printf '%s\\n' 'Shellfox: directory does not exist or is inaccessible.' >&2; exit 1; }
  mode=start
else printf '%s\\n' 'Usage: shellfox start [path] | shellfox update [--check]' >&2; exit 2; fi
if [ ! -x ${shQuote(options.executable)} ]; then printf '%s\\n' 'Shellfox: application executable is missing.' >&2; exit 1; fi
${Object.entries(options.launchEnv??{}).map(([key,value])=>`export ${key}=${shQuote(value)}`).join('\n')}
launch() {
  if command -v setsid >/dev/null 2>&1; then exec setsid --fork --wait ${target} "$@" </dev/null >/dev/null 2>&1
  else exec nohup ${target} "$@" </dev/null >/dev/null 2>&1; fi
}
# Redirect on exec inside the async child, not on a redirected shell function:
# the latter can keep saved copies of the caller's output pipes open.
if [ "$mode" = start ]; then launch start "$folder" &
else launch & fi
child=$!
# A warm instance normally acknowledges by exiting; a cold GUI keeps running.
# Bound this check, never wait for a GUI or shell lifetime and never signal it.
i=0
while [ "$i" -lt 5 ] && kill -0 "$child" 2>/dev/null; do sleep 0.1; i=$((i+1)); done
if ! kill -0 "$child" 2>/dev/null; then wait "$child" || { printf '%s\\n' 'Shellfox: application rejected the launch.' >&2; exit 1; }; fi
if [ "$mode" = start ]; then printf 'Shellfox: started session in %s\\n' "$folder"; fi
exit 0
`;
}
export class LinuxCliIntegration implements CliPort {
  readonly binDir: string;
  readonly launcher: string;
  readonly packaged: boolean;
  constructor(private readonly options: LinuxCliOptions) {
    this.packaged=!options.appPath&&!options.prefixArgs?.length&&options.executable==='/usr/lib/shellfox/shellfox';
    this.binDir=this.packaged?'/usr/bin':path.join(options.home??homedir(),'.local','bin');
    this.launcher=this.packaged?(options.systemLauncher??'/usr/bin/shellfox'):path.join(this.binDir,'shellfox');
  }
  private state(installed: boolean): CliIntegrationDto { const onPath=(this.options.env??process.env).PATH?.split(':').some(dir=>path.resolve(dir)===path.resolve(this.binDir));return {supported:true,installed,command:'shellfox start <path>',reason:this.packaged?'Installed with the package':onPath?null:'Add ~/.local/bin to PATH (or log out and in)'}; }
  async get(): Promise<Result<CliIntegrationDto>> {
    let installed=false;try { await access(this.launcher,constants.X_OK);const content=await readFile(this.launcher,'utf8');installed=this.packaged?ownsLinuxFile(content):content===linuxLauncher(this.options); } catch { /* Missing file is not installed. */ }
    return success(this.state(installed));
  }
  async set(installed: boolean): Promise<Result<CliIntegrationDto>> {
    if(this.packaged) return installed?this.get():failure('UNSUPPORTED','Installed with the package. Remove the package to remove /usr/bin/shellfox.');
    try {
      const metadata=await lstat(this.launcher).catch(error=>{if(error.code==='ENOENT')return null;throw error;});
      if(metadata?.isSymbolicLink()) return failure('AUTH_FAILED','The existing shellfox launcher is a foreign symbolic link.');
      let current:string|null=null;try{current=await readFile(this.launcher,'utf8');}catch(error){if((error as NodeJS.ErrnoException).code!=='ENOENT')throw error;}
      if(current!==null&&!ownsLinuxFile(current)) return failure('AUTH_FAILED','The existing shellfox launcher is not owned by Shellfox.');
      if(installed){await mkdir(this.binDir,{recursive:true});await writeFile(this.launcher,linuxLauncher(this.options));await chmod(this.launcher,0o755);}
      else if(current!==null)await unlink(this.launcher);
      return this.get();
    }catch{return failure('STORAGE_FAILED','Shellfox launcher could not be updated.',true);}
  }
}
