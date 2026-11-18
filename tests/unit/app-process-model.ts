import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import { modelEndpointCommand, PermissionModelBoundaryError } from './endpoint-permission-model';
import { modelDirectoryDrive } from './directory-drive-model';
const blocked: string[] = [];
export function denyAppProcess(api: string): never { blocked.push(api); throw new Error('UNIT_APP_PROCESS_DENIED: ' + api); }
export function takeBlockedProcesses(): string[] { return blocked.splice(0); }
/** Inert event/stream model only. No actual ChildProcess constructor or OS handle. */
class ModelChild extends EventEmitter {
  stdin = new PassThrough(); stdout = new PassThrough(); stderr = new PassThrough();
  pid: number | undefined; exitCode: number | null = null; signalCode: string | null = null;
  spawn() { return denyAppProcess('ChildProcess.spawn'); }
  kill() { return denyAppProcess('unscripted model kill'); }
}
const execFile = Object.assign(() => denyAppProcess('execFile callback'), {
  [Symbol.for('nodejs.util.promisify.custom')]: async (file: unknown, args: unknown, options: unknown) => {
    const drive = modelDirectoryDrive(file, args, options);
    if (drive) return drive;
    try { return await modelEndpointCommand(file, args, options); }
    catch (error) {
      if (error instanceof PermissionModelBoundaryError) denyAppProcess('unregistered execFile protocol');
      throw error;
    }
  },
});
/** Local child models can extend these denials without loading the real builtin. */
const api = {
  ChildProcess: ModelChild, execFile,
  spawn: () => denyAppProcess('spawn'), fork: () => denyAppProcess('fork'),
  exec: () => denyAppProcess('exec'), execSync: () => denyAppProcess('execSync'),
  spawnSync: () => denyAppProcess('spawnSync'), execFileSync: () => denyAppProcess('execFileSync'),
};
export const unitChildProcess = { ...api, default: api };
const moduleApi = { createRequire: () => denyAppProcess('CJS actor graph HOLD') };
export const unitModule = { ...moduleApi, default: moduleApi };
export const unitPty = { spawn: () => denyAppProcess('PTY HOLD') };
export const unitAgent = {
  agentEndpoints: () => denyAppProcess('agent endpoints'),
  availableAgent: () => denyAppProcess('agent identities'),
};
