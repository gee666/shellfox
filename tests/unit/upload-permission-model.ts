import { createHash } from 'node:crypto';
import { lstat } from 'node:fs/promises';
import path from 'node:path';
import type { PermissionCommand, protectWindowsUploadDirectory } from '../../src/main/remote/upload-permissions';
import { denyAppProcess } from './app-process-model';

const projectTmp = path.resolve('tmp');
const roots = new Map<string, number>();
const sid = 'S-1-5-21-101-102-103-1000';
const daclScriptSha256 = 'd50acf0769165afc777f0516adb2607f07abde00474b454199522331eeeec108';

export function registerFixtureUploadRoot(root: string): void {
  if (process.platform !== 'win32') return;
  if (path.resolve(root) !== root || path.basename(root) !== 'uploads' || path.dirname(path.dirname(root)) !== projectTmp ||
      roots.has(root) || roots.size >= 64) throw new Error('Invalid upload permission fixture');
  // At most one initialization and a restart, with an extra slot for the direct storage fixture.
  roots.set(root, 3);
}

export function finishFixtureUploadRoots(): void { roots.clear(); }

export function fixtureUploadPermissions(protect: typeof protectWindowsUploadDirectory): typeof protectWindowsUploadDirectory {
  return async (directory, run) => {
    if (run) return protect(directory, run);
    const remaining = roots.get(directory) ?? 0;
    if (remaining <= 0) denyAppProcess('unregistered upload permission fixture');
    roots.set(directory, remaining - 1);
    const info = await lstat(directory);
    if (!info.isDirectory() || info.isSymbolicLink()) throw new Error('Invalid upload fixture directory');
    const system = path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32');
    let stage = 0;
    const command: PermissionCommand = async (file, args, input) => {
      if (stage === 0 && file === path.join(system, 'whoami.exe') &&
          same(args, ['/user', '/fo', 'csv', '/nh']) && input === undefined) {
        stage = 1;
        return `"fixture","${sid}"\r\n`;
      }
      if (stage === 1 && file === path.join(system, 'WindowsPowerShell', 'v1.0', 'powershell.exe') &&
          args.length === 5 && same(args.slice(0, 4), ['-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand']) &&
          input === JSON.stringify({ directory, sid }) && args[4]!.length <= 8192 &&
          createHash('sha256').update(Buffer.from(args[4]!, 'base64').toString('utf16le')).digest('hex') === daclScriptSha256) {
        stage = 2;
        return '';
      }
      return denyAppProcess('unmodeled upload permission command');
    };
    await protect(directory, command);
    if (stage !== 2) denyAppProcess('incomplete upload permission model');
  };
}

function same(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((value, index) => value === b[index]);
}
