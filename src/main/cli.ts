import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { z } from 'zod';
import { idSchema, pathSchema } from '../shared/schemas';
import { failure, success } from '../shared/contracts';
import type { Result, SessionDto } from '../shared/contracts';

export type CliRequest = { version: 1; kind: 'show' } | { version: 1; kind: 'new-session'; requestId: string; cwd: string };
export const cliRequestSchema = z.discriminatedUnion('kind', [
  z.object({ version: z.literal(1), kind: z.literal('show') }).strict(),
  z.object({ version: z.literal(1), kind: z.literal('new-session'), requestId: idSchema, cwd: pathSchema }).strict(),
]);
// Electron/Playwright runtime switches are not product flags or directory values.
export function productArguments(args: string[]): string[] {
  return args.filter(arg => !/^--(?:inspect(?:-brk)?|remote-debugging-port)=\d+$/.test(arg));
}
export interface ParsedCli { request: CliRequest; userData?: string; backend?: 'fake' | 'real' }
export function parseCli(args: string[], options: { testMode?: boolean; testBuild?: boolean; projectRoot?: string; cwd?: string } = {}): Result<ParsedCli> {
  let newSession = false;
  let cwd: string | undefined;
  let userData: string | undefined;
  let backend: 'fake' | 'real' | undefined;
  let start = false;
  const seen = new Set<string>();
  for (let i = 0; i < args.length; i++) {
    const flag = args[i];
    if (seen.has(flag)) return failure('VALIDATION', 'Duplicate CLI flag');
    seen.add(flag);
    if (flag === 'start') {
      if (newSession || cwd !== undefined || start) return failure('VALIDATION', 'Do not combine start with legacy session flags.');
      start = newSession = true;
      const target = args[i + 1] && !['--test-user-data', '--test-backend', '--new-session', '--cwd'].includes(args[i + 1]!) ? args[++i]! : '.';
      const base = options.cwd ?? process.cwd();
      cwd = (/^[a-z]:[\\/]/i.test(base) || /^\\\\/.test(base) ? path.win32 : path.posix).resolve(base, target);
      continue;
    }
    if (start && (flag === '--new-session' || flag === '--cwd')) return failure('VALIDATION', 'Do not combine start with legacy session flags.');
    if (flag === '--new-session') { newSession = true; continue; }
    if (flag === '--cwd') { cwd = args[++i]; if (!cwd || cwd.startsWith('--')) return failure('VALIDATION', 'Missing directory'); continue; }
    if (flag === '--test-user-data' || flag === '--test-backend') {
      if (!options.testMode) return failure('VALIDATION', 'Test flags require explicit test mode');
      const value = args[++i];
      if (flag === '--test-user-data') {
        if (!value || !pathSchema.safeParse(value).success || !options.projectRoot) return failure('VALIDATION', 'Invalid test data directory');
        const root = path.resolve(options.projectRoot, 'tmp');
        const relative = path.relative(root, path.resolve(value));
        if (!relative || relative.startsWith('..') || path.isAbsolute(relative)) return failure('VALIDATION', 'Test data must be inside project tmp');
        userData = path.resolve(value);
      } else {
        if (value !== 'fake' && value !== 'real') return failure('VALIDATION', 'Invalid test backend');
        if (value === 'fake' && !options.testBuild) return failure('VALIDATION', 'Fake backend is not in the production build');
        backend = value;
      }
      continue;
    }
    return failure('VALIDATION', 'Unknown product flag');
  }
  if (newSession !== (cwd !== undefined)) return failure('VALIDATION', '--new-session and --cwd must be supplied together');
  if (cwd !== undefined && !pathSchema.safeParse(cwd).success) return failure('VALIDATION', 'Expected an absolute local directory');
  if (options.testMode && (!userData || !backend)) return failure('VALIDATION', 'Test mode requires isolated user data and an explicit backend');
  return success({ request: cwd ? { version: 1, kind: 'new-session', requestId: randomUUID(), cwd } : { version: 1, kind: 'show' }, ...(userData ? { userData } : {}), ...(backend ? { backend } : {}) });
}
export function validateForwarded(input: unknown): Result<CliRequest> {
  const parsed = cliRequestSchema.safeParse(input);
  return parsed.success ? success(parsed.data) : failure('VALIDATION', 'Invalid forwarded CLI request');
}
interface ExternalSessionHost {
  createSession(input: { cwd: string; requestId: string; title?: string }): Promise<Result<SessionDto>>;
  selectSession(sessionId: string): void;
}
interface MainWindowHost { isDestroyed(): boolean; isMinimized(): boolean; restore(): void; show(): void; focus(): void }
/** Used for both cold-start and second-instance requests, after the renderer loads. */
export async function handleCliRequest(request: CliRequest, service: ExternalSessionHost, window: MainWindowHost, presentWindow = true): Promise<Result<{ handled: true }>> {
  if (window.isDestroyed()) return failure('TARGET_LOST', 'The manager window is closed.');
  if (request.kind === 'new-session') {
    const paths = /^[a-z]:[\\/]/i.test(request.cwd) || /^\\\\/.test(request.cwd) ? path.win32 : path.posix;
    const cwd = paths.normalize(request.cwd);
    const title = (paths.basename(cwd) || cwd).slice(0, 200);
    // Embedded createSession already opens exactly one tab with the default profile.
    // Legacy adapters retain their own create/launch semantics.
    const result = await service.createSession({ cwd, title, requestId: request.requestId });
    if (!result.ok) return result;
    service.selectSession(result.value.id);
  }
  if (presentWindow && !window.isDestroyed()) {
    if (window.isMinimized()) window.restore();
    window.show(); window.focus();
  }
  return success({ handled: true });
}
export class EarlyRequestQueue {
  private pending: CliRequest[] = [];
  private seen = new Set<string>();
  private handler?: (request: CliRequest) => Promise<void>;
  private drainPromise: Promise<void> = Promise.resolve();
  enqueue(input: unknown): Result<{ queued: true }> {
    const result = validateForwarded(input);
    if (!result.ok) return result;
    const request = result.value;
    if (request.kind === 'new-session') {
      if (this.seen.has(request.requestId)) return success({ queued: true });
      this.seen.add(request.requestId);
    }
    this.pending.push(request);
    this.drain();
    return success({ queued: true });
  }
  ready(handler: (request: CliRequest) => Promise<void>): void { this.handler = handler; this.drain(); }
  private drain(): void {
    if (!this.handler) return;
    this.drainPromise = this.drainPromise.then(async () => {
      while (this.pending.length && this.handler) await this.handler(this.pending.shift()!);
    }).catch(() => { this.drain(); });
  }
  async idle(): Promise<void> { await this.drainPromise; }
}
