import { mkdir, realpath } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import path from 'node:path';
import { z } from 'zod';
import type { HostMode } from '../shared/host-contracts';
import { hostResourcePaths, type HostResourceOptions } from './paths';
import { legacyHostIsRunning } from './legacy-owner';

export interface HostOwnership {
  readonly userData: string; readonly mode: HostMode;
  readonly lost: Error | undefined;
  assertOwned(): void;
  release(): Promise<void>;
  onLost(listener: (error: Error) => void): () => void;
}
export interface HostOwnershipOptions extends HostResourceOptions { helperPath?: string }
const resultSchema = z.object({ locked: z.boolean() }).strict();
/** The offline native helper holds flock/LockFileEx until our stdin pipe closes. */
export async function acquireHostOwnership(userData: string, mode: HostMode = 'desktop', options: HostOwnershipOptions = {}): Promise<HostOwnership | null> {
  await mkdir(userData, { recursive: true, mode: 0o700 });
  const canonical = await realpath(path.resolve(userData));
  const helperPath = options.helperPath ?? hostResourcePaths(options).tailscale;
  const child = spawn(helperPath, ['--ownership', canonical], { windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
  child.stdin.on('error', () => {});
  const listeners = new Set<(error: Error) => void>();
  let lost: Error | undefined, released = false, releasePromise: Promise<void> | undefined;
  const closed = new Promise<void>(resolve => child.once('close', () => resolve()));
  child.on('error', error => { lost = error; });
  child.once('exit', () => {
    if (released) return;
    lost = new Error('Shellfox ownership helper exited. Stop this host before accessing its database again.');
    for (const listener of listeners) { try { listener(lost); } catch { /* Other owners still need the loss notification. */ } }
  });
  const lock: HostOwnership = {
    userData: canonical, mode,
    get lost() { return lost; },
    assertOwned() { if (lost) throw lost; if (released) throw new Error('Host ownership was already released.'); },
    release() {
      return releasePromise ??= (async () => {
        released = true; listeners.clear(); child.stdin.end();
        const timer = setTimeout(() => child.kill(), 5000); timer.unref();
        await closed; clearTimeout(timer);
      })();
    },
    onLost(listener) { listeners.add(listener); if (lost) listener(lost); return () => { listeners.delete(listener); }; },
  };
  try {
    const locked = await new Promise<boolean>((resolve, reject) => {
      let bytes = 0, text = '', done = false;
      const finish = (error?: Error, value?: boolean) => { if (done) return; done = true; clearTimeout(timer); if (error) reject(error); else resolve(value === true); };
      const timer = setTimeout(() => finish(new Error('Shellfox ownership helper timed out.')), 8000);
      child.once('error', error => finish(new Error('Shellfox ownership helper could not start: ' + helperPath, { cause: error })));
      // A busy helper exits immediately after writing its reply. Its stdout may
      // still be draining at exit; close is the actual end of the handshake.
      child.once('close', () => { if (!done) finish(new Error('Shellfox ownership helper exited before confirming the lock.')); });
      child.stderr.on('data', (chunk: Buffer) => { bytes += chunk.length; if (bytes > 4096) finish(new Error('Ownership helper exceeded its output limit.')); });
      child.stdout.on('data', (chunk: Buffer) => {
        bytes += chunk.length;
        if (bytes > 4096) { finish(new Error('Ownership helper exceeded its output limit.')); return; }
        if (done) return;
        text += chunk.toString('utf8');
        if (!text.includes('\n')) return;
        try { const parsed = resultSchema.parse(JSON.parse(text.trim())); finish(undefined, parsed.locked); }
        catch { finish(new Error('Invalid Shellfox ownership helper response.')); }
      });
    });
    if (!locked) { await lock.release(); return null; }
    if (lost) throw lost;
    if (await legacyHostIsRunning(canonical)) { await lock.release(); return null; }
    if (lost) throw lost;
    return lock;
  } catch (error) { await lock.release(); throw error; }
}
export async function isHostRunning(userData: string, options: HostOwnershipOptions = {}): Promise<boolean> {
  const lock = await acquireHostOwnership(userData, 'desktop', options);
  if (!lock) return true;
  await lock.release(); return false;
}
