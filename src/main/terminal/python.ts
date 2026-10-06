import path from 'node:path';
import { access, stat } from 'node:fs/promises';
import { constants } from 'node:fs';
import { execFile } from 'node:child_process';
import type { NativeProbe } from '../../shared/contracts';
import { PIDFD_PREFLIGHT, PIDFD_READY } from './pidfd-preflight';
export type PythonProbe = NonNullable<NativeProbe['python']>;
export interface PythonOptions { env?: NodeJS.ProcessEnv; cwd?: string; exists?: (file: string) => Promise<boolean>; run?: (file: string, args: string[]) => Promise<Buffer | string> }
const executable = async (file: string) => { try { await access(file, constants.X_OK); return (await stat(file)).isFile(); } catch { return false; } };
const execute = (file: string, args: string[]): Promise<string> => new Promise((resolve,reject) => execFile(file,args,{encoding:'utf8',timeout:8000,maxBuffer:65536},(error,stdout)=>error?reject(error):resolve(stdout)));
export async function resolvePython(configured: string | null = null, options: PythonOptions = {}, explicit = false): Promise<PythonProbe> {
  const exists=options.exists??executable, run=options.run??execute, env=options.env??process.env;
  const candidates=explicit ? (configured?[configured]:[]) : [...new Set([...(configured?[configured]:[]),'/usr/bin/python3',...(env.PATH===undefined?[]:env.PATH.split(':')).map(dir=>path.posix.resolve(options.cwd??process.cwd(),dir||'.','python3'))])];
  let detected: string | null=null, reason: string | null=null;
  for(const candidate of candidates) {
    if(!candidate.startsWith('/') || /[\0\r\n]/.test(candidate) || !await exists(candidate)) continue;
    detected??=candidate;
    try { if((await run(candidate,['-c',PIDFD_PREFLIGHT])).toString().trim()===PIDFD_READY) return {detected:candidate,usable:true,reason:null}; }
    catch { /* A failed preflight does not authorize a numeric-PID kill. */ }
    reason='Python 3 was found, but pidfd process access/signaling is unavailable. Check Python 3.9+, the Linux kernel and sandbox restrictions.';
  }
  return {detected,usable:false,reason:reason??(explicit?'The path must be an absolute executable Python 3 path.':'Python 3 not found. Set its path in Settings → Python.')};
}
