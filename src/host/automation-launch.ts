import { spawn, type ChildProcess, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { writeFileSync, rmSync } from 'node:fs';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { z } from 'zod';
import type { AutomationStart } from '../shared/automation-schemas';
import { elevatedHelperPath } from '../main/terminal/windows-elevated';
import { hostResourcePaths } from './paths';
import { resolveAutomationExecutable } from './automation-executable';

function hasPipes(child: ChildProcess): child is ChildProcessWithoutNullStreams {
  return child.stdin !== null && child.stdout !== null && child.stderr !== null;
}
const exitSchema = z.object({ type: z.literal('exit'), code: z.number().int().nullable(), signal: z.string().nullable() }).strict();
const messageSchema = z.union([exitSchema, z.object({ type: z.literal('ready') }).strict(), z.object({ type: z.literal('error'), message: z.string() }).strict()]);
// This supervisor remains the process-group leader until the host kills the
// group. Its live PID prevents recycling the group ID during normal cleanup.
// The target inherits the three actual pipe handles, not a terminal or text relay.
const UNIX_SUPERVISOR = `
const {spawn} = require('node:child_process');
let launched = false;
const stop = () => { try { process.kill(-process.pid, 'SIGKILL'); } catch { process.exit(1); } };
process.on('disconnect', stop);
process.once('message', input => {
  const child = spawn(input.executable, input.args, {cwd:input.cwd,env:input.env,stdio:'inherit',shell:false});
  child.once('spawn',()=>{launched=true;process.send({type:'ready'});});
  child.once('error',error=>{process.send({type:'error',message:error.message});});
  child.once('exit',(code,signal)=>{if(launched)process.send({type:'exit',code,signal});});
});
`;
export interface AutomationLaunch {
  child: ChildProcessWithoutNullStreams;
  ready: Promise<void>;
  outcome(): { code: number | null; signal: string | null } | undefined;
}
/** Acquire native containment before starting a target. The host never kills a recycled PID. */
export function launchAutomation(input: AutomationStart, directory: string): AutomationLaunch {
  if (process.platform !== 'win32') {
    const env = { ...process.env, ...(process.versions.electron ? { ELECTRON_RUN_AS_NODE: '1' } : {}) };
    const child = spawn(process.execPath, ['-e', UNIX_SUPERVISOR], {
      env, stdio: ['pipe', 'pipe', 'pipe', 'ipc'], detached: true, shell: false,
    });
    if (!hasPipes(child)) { child.kill('SIGKILL'); throw new Error('Automation pipes were not created.'); }
    let result: { code: number | null; signal: string | null } | undefined;
    const ready = new Promise<void>((resolve, reject) => {
      child.on('error', reject);
      child.once('close', () => reject(new Error('The automation supervisor exited before launch.')));
      child.on('message', raw => {
        const parsed = messageSchema.safeParse(raw);
        if (!parsed.success) { reject(new Error('Invalid automation supervisor response.')); return; }
        const message = parsed.data;
        const pid = child.pid;
        if (!pid) { reject(new Error('The automation supervisor has no process identity.')); return; }
        if (message.type === 'ready') resolve();
        else if (message.type === 'error') { reject(new Error(message.message)); try { process.kill(-pid, 'SIGKILL'); } catch { /* Already exited. */ } }
        else {
          result = { code: message.code, signal: message.signal };
          // The supervisor is still alive. Kill its exact group before it can be reaped.
          try { process.kill(-pid, 'SIGKILL'); } catch { child.kill('SIGKILL'); }
        }
      });
      child.once('spawn', () => child.send({ executable: input.executable, args: input.args, cwd: input.cwd, env: input.env }, error => { if (error) reject(error); }));
    });
    return { child, ready, outcome: () => result };
  }
  // The host endpoint directory already has a protected current-user ACL. Never
  // put environment values in argv or a public temporary directory.
  const executable = resolveAutomationExecutable(input.executable, input.cwd, input.env);
  const file = path.join(directory, `automation-${randomUUID()}.json`);
  const remove = () => {
    try { rmSync(file, { force: true }); }
    catch { console.error('Shellfox could not remove a private automation launch file.'); }
  };
  try {
    writeFileSync(file, JSON.stringify({ executable, args: input.args, cwd: input.cwd, env: input.env }), { flag: 'wx', mode: 0o600 });
    const child = spawn(elevatedHelperPath(hostResourcePaths().helpers), ['pipe-run', file], {
      stdio: 'pipe', windowsHide: true, shell: false,
    });
    child.once('close', remove);
    const ready = new Promise<void>((resolve, reject) => { child.once('spawn', resolve); child.on('error', reject); });
    return { child, ready, outcome: () => undefined };
  } catch (error) { remove(); throw error; }
}
