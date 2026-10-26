import path from 'node:path';
import { homedir } from 'node:os';
import { access, readFile, writeFile, mkdir, chmod, unlink, lstat } from 'node:fs/promises';
import { constants } from 'node:fs';
import type { CliIntegrationDto, Result } from '../../shared/contracts';
import { failure, success } from '../../shared/contracts';
import { posixHelper } from './cli-helper';
import { launchConfig } from './shellfox-cli-launch';
import type { CliPort } from './shellfox-cli';
import updateScript from '../update/shellfox-update.sh';
export const LINUX_OWNER = 'Shellfox/linux-v1';
export const ownsLinuxFile = (content: string) => content.split(/\r?\n/).includes('# ' + LINUX_OWNER);
export interface LinuxCliOptions { executable: string; appPath?: string; cliHelper?: string; userData?: string; home?: string; env?: NodeJS.ProcessEnv; prefixArgs?: string[]; launchEnv?: Record<string,string>; systemLauncher?: string; resourcesPath?: string; projectRoot?: string }
export const shQuote = (value: string) => "'"+value.replaceAll("'","'\\''")+"'";
export function linuxLauncher(options: LinuxCliOptions): string {
  return `#!/bin/sh
# ${LINUX_OWNER}
export SHELLFOX_CLI_LAUNCH=${shQuote(launchConfig(options.executable, [...(options.appPath ? [options.appPath] : []), ...(options.prefixArgs ?? [])], options.launchEnv ?? {}))}
case "$#:\${1:-}:\${2:-}" in
  1:update:|2:update:--check)
    unset ELECTRON_RUN_AS_NODE
    shift
    shellfox_update_script=${shQuote(updateScript)}
    SHELLFOX_APP_EXECUTABLE=${shQuote(options.executable)} exec /bin/sh -c "$shellfox_update_script" shellfox-update "$@"
    ;;
esac
${posixHelper({ ...options, userDataIsDefault: true }, false)}`;
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
  private state(installed: boolean): CliIntegrationDto { const onPath=(this.options.env??process.env).PATH?.split(':').some(dir=>path.resolve(dir)===path.resolve(this.binDir));return {supported:true,installed,command:'shellfox',reason:this.packaged?'Installed with the package':onPath?null:'Add ~/.local/bin to PATH (or log out and in)'}; }
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
