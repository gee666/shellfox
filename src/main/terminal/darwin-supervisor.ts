import { randomBytes } from 'node:crypto';
import { connect } from 'node:net';
import { mkdtemp, chmod, realpath, stat, rmdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { z } from 'zod';
import type { TerminalProfileDto } from '../../shared/contracts';
import { macTrackingHelperPath, type TrackingHelperOptions } from './tracking';
import { execute, type Execute } from './profiles';

export interface SupervisorCapability { available: boolean; reason: string | null; helperPath: string | null }
export function darwinSupervisorPath(options: TrackingHelperOptions = {}): string {
  return path.join(path.dirname(macTrackingHelperPath(options)), 'shellfox-terminal-supervisor');
}
export async function getDarwinSupervisorCapability(options: TrackingHelperOptions = {}, run: Execute = execute): Promise<SupervisorCapability> {
  if ((options.platform ?? process.platform) !== 'darwin') return { available: false, reason: 'Darwin supervisor requires macOS.', helperPath: null };
  let helperPath: string;
  try { helperPath = darwinSupervisorPath(options); } catch { return { available: false, reason: 'Packaged Darwin supervisor resources path is unavailable.', helperPath: null }; }
  try {
    const response = z.object({ version: z.literal(1), platform: z.literal('darwin'), arch: z.enum(['x64', 'arm64']), available: z.boolean(), reason: z.string().max(1000).nullable(), ownedSession: z.literal(true), guardians: z.literal(true), termination: z.literal(true) }).strict().parse(JSON.parse((await run(helperPath, ['--capabilities'])).toString('utf8')));
    if (response.arch !== (options.arch ?? process.arch) || response.available && response.reason !== null || response.reason && /[\u0000-\u001f\u007f]/.test(response.reason)) throw new Error('Incompatible supervisor preflight.');
    return { available: response.available, reason: response.available ? null : response.reason ?? 'Native supervisor preflight refused.', helperPath };
  } catch { return { available: false, reason: `The bundled Darwin session supervisor is missing, incompatible, or failed native preflight: ${helperPath}`, helperPath }; }
}
export const supervisorStatusSchema = z.object({ version: z.literal(1), ok: z.boolean(), state: z.enum(['open', 'closing', 'closed']), supervisorPid: z.number().int().positive().max(0x7fffffff), shellPid: z.number().int().nonnegative().max(0x7fffffff), shellBirth: z.string().max(80).regex(/^(?:darwin:\d+:\d{6})?$/), shellAlive: z.boolean(), exitCode: z.number().int().nullable(), reason: z.string().max(1000).nullable().refine(s => s === null || !/[\u0000-\u001f\u007f]/.test(s)) }).strict().refine(s => s.state !== 'closed' || !s.shellAlive);
export type SupervisorStatus = z.infer<typeof supervisorStatusSchema>;
export interface SupervisorControl {
  ready(supervisorPid: number): Promise<SupervisorStatus>;
  close(): Promise<SupervisorStatus>;
  disposeConfirmed(): Promise<void>;
}
export interface SupervisorLaunch {
  file: string; args: string[]; cwd: string; env: Record<string, string>; control: SupervisorControl;
}
/** A single authenticated private control connection, bounded framing and no target kill on timeout. */
export class DarwinSupervisorControl implements SupervisorControl {
  private supervisorPid: number | null = null;
  private shellPid: number | null = null;
  private shellBirth: string | null = null;
  constructor(private readonly socketPath: string, private readonly token: string, private readonly directory?: string, private readonly timeoutMs = 15000) {
    if (!/^[a-f0-9]{64}$/.test(token) || !path.isAbsolute(socketPath) || Buffer.byteLength(socketPath) >= 104) throw new Error('Invalid private Darwin control endpoint.');
  }
  private request(command: 'STATUS' | 'CLOSE'): Promise<SupervisorStatus> {
    return new Promise((resolve, reject) => {
      const socket = connect({ path: this.socketPath }); let buffer = Buffer.alloc(0), done = false;
      const finish = (error: Error | null, response?: SupervisorStatus) => { if (done) return; done = true; clearTimeout(timer); socket.destroy(); error ? reject(error) : resolve(response!); };
      const timer = setTimeout(() => finish(new Error('Darwin supervisor request timed out; its owned process was not killed.')), this.timeoutMs);
      socket.once('connect', () => socket.write(`1\t${this.token}\t${command}\n`));
      socket.on('data', data => {
        buffer = Buffer.concat([buffer, data]);
        if (buffer.length > 2048) { finish(new Error('Supervisor control frame exceeded its bound.')); return; }
        const newline = buffer.indexOf(10); if (newline < 0) return;
        try {
          if (newline !== buffer.length - 1) throw new Error('Multiple control frames are not permitted.');
          const response = supervisorStatusSchema.parse(JSON.parse(new TextDecoder('utf8', { fatal: true }).decode(buffer.subarray(0, newline))));
          if (this.supervisorPid !== null && response.supervisorPid !== this.supervisorPid || this.shellPid !== null && response.shellPid !== this.shellPid || this.shellBirth !== null && response.shellBirth !== this.shellBirth) throw new Error('Supervisor or shell identity changed.');
          finish(null, response);
        } catch { finish(new Error('Invalid or mismatched supervisor control reply.')); }
      });
      socket.once('error', () => finish(new Error('Darwin supervisor control connection failed.')));
      socket.once('end', () => { if (!done) finish(new Error('Darwin supervisor ended without a verified reply.')); });
    });
  }
  async ready(supervisorPid: number): Promise<SupervisorStatus> {
    this.supervisorPid = supervisorPid;
    const deadline = Date.now() + this.timeoutMs;
    let last: Error | undefined;
    do {
      try {
        const response = await this.request('STATUS');
        if (!response.ok || response.state !== 'open' || !response.shellAlive || !response.shellBirth || response.shellPid <= 0 || response.shellPid === supervisorPid) throw new Error(response.reason ?? 'Supervisor shell startup was not authenticated.');
        this.shellPid = response.shellPid; this.shellBirth = response.shellBirth; return response;
      } catch (error) { last = error as Error; }
      await new Promise(resolve => setTimeout(resolve, 20));
    } while (Date.now() < deadline);
    throw last ?? new Error('Supervisor startup timed out.');
  }
  async close(): Promise<SupervisorStatus> {
    const response = await this.request('CLOSE');
    if (!response.ok || response.state !== 'closed' || response.shellAlive) throw new Error(response.reason ?? 'Owned Darwin session cleanup was not confirmed.');
    return response;
  }
  async disposeConfirmed(): Promise<void> { if (this.directory) { try { await rmdir(this.directory); } catch { /* Never recursively remove a replaced/occupied endpoint directory. */ } } }
}
export async function prepareDarwinSupervisor(profile: TerminalProfileDto, cwd: string, marker: string, options: TrackingHelperOptions & { runtimeDirectory?: string; run?: Execute } = {}): Promise<SupervisorLaunch> {
  const capability = await getDarwinSupervisorCapability(options, options.run ?? execute);
  if (!capability.available || !capability.helperPath) throw new Error(capability.reason ?? 'Darwin supervisor unavailable.');
  const base = await realpath(options.runtimeDirectory ?? tmpdir());
  const created = await mkdtemp(path.join(base, 'shellfox-')); await chmod(created, 0o700);
  const directory = await realpath(created), info = await stat(directory);
  if (info.uid !== process.getuid?.() || (info.mode & 0o777) !== 0o700) { await rmdir(directory); throw new Error('Private control directory ownership could not be enforced.'); }
  const socketPath = path.join(directory, 's');
  if (Buffer.byteLength(socketPath) >= 104) { await rmdir(directory); throw new Error('The private Darwin socket path is too long.'); }
  const token = randomBytes(32).toString('hex');
  return { file: capability.helperPath, args: ['--supervise', socketPath, profile.executable, ...profile.args], cwd, env: { SHELLFOX_SUPERVISOR_TOKEN: token, SHELLFOX_TERMINAL_MARKER: marker }, control: new DarwinSupervisorControl(socketPath, token, directory) };
}
