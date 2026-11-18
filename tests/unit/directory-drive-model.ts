import { AsyncLocalStorage } from 'node:async_hooks';
import { realpath } from 'node:fs/promises';
import path from 'node:path';

const fixtureTmp = path.resolve('tmp');
interface Probe { remaining: number; executable: string; flag: string }
const probes = new Map<string, Probe>();
const validating = new AsyncLocalStorage<string>();

/** Register each expected drive query for an owned directory. No modeled result implies real drive acceptance. */
export async function registerDirectoryDriveChecks(root: string, targets: readonly string[]): Promise<void> {
  if (process.platform !== 'win32') return;
  if (path.dirname(root) !== fixtureTmp || targets.length > 8 || probes.size + targets.length > 64) throw new Error('Invalid directory drive fixture');
  const canonical = await realpath(root), drive = /^([A-Z]):[\\/]/i.exec(canonical)?.[1]?.toUpperCase();
  if (!drive) throw new Error('Directory fixture root has no drive letter');
  const executable = path.join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe');
  const script = `[Console]::Write([System.IO.DriveInfo]::new('${drive}:\\').DriveType.ToString())`;
  const flag = Buffer.from(script, 'utf16le').toString('base64');
  for (const target of targets) {
    const normalized = path.resolve(target), relative = path.relative(root, normalized);
    if (target !== normalized || path.isAbsolute(relative) || relative.split(path.sep)[0] === '..') throw new Error('Drive probe outside its fixture');
    const prior = probes.get(normalized);
    if (prior && (prior.executable !== executable || prior.flag !== flag)) throw new Error('Conflicting drive probe fixture');
    if (prior) prior.remaining++;
    else probes.set(normalized, { executable, flag, remaining: 1 });
  }
}

export function withDirectoryDriveCheck<T>(target: string, validate: () => Promise<T>): Promise<T> {
  return validating.run(target, validate);
}

export function modelDirectoryDrive(file: unknown, args: unknown, options: unknown) {
  const target = validating.getStore(), probe = target === undefined ? undefined : probes.get(target);
  if (!probe || probe.remaining === 0 || file !== probe.executable || !Array.isArray(args)) return;
  const expected = ['-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand', probe.flag];
  if (args.length !== expected.length || expected.some((value, index) => value !== args[index])) return;
  if (typeof options !== 'object' || options === null || Object.keys(options).length !== 3 ||
    !('windowsHide' in options && options.windowsHide === true && 'timeout' in options && options.timeout === 5000 &&
      'maxBuffer' in options && options.maxBuffer === 4096)) return;
  probe.remaining--;
  return { stdout: 'Fixed', stderr: '' };
}

export function finishDirectoryDriveChecks(): void {
  const pending = [...probes.values()].some(probe => probe.remaining !== 0);
  probes.clear();
  if (pending) throw new Error('Unconsumed directory drive checks');
}
