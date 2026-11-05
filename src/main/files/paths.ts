import path from 'node:path';
import { hostname, homedir } from 'node:os';
import type { FileLocation } from '../../shared/file-contracts';
import { fileLocationSchema } from '../../shared/file-schemas';
import { parseWslUnc } from '../../shared/wsl-path';

export class FileError extends Error {
  constructor(readonly code: 'VALIDATION' | 'NOT_FOUND' | 'UNSUPPORTED' | 'ACCESS_DENIED' | 'FILE_CONFLICT', message: string) { super(message); }
}
export interface FileSessionContext {
  session(id: string): { cwd: string } | undefined;
  tab(id: string): { sessionId: string; cwd: string; terminal?: { profileId: string } | null } | undefined;
  resolveDirectory?(id: string): Promise<string>;
  terminal?(id: string): { cwd: string; directoryUrl?: string | null; root: { environment: 'local' | 'wsl'; distro: string | null } } | undefined;
}
export interface FileTarget {
  native: string;
  display(native: string): string;
}
export async function resolveFileTarget(context: FileSessionContext, input: FileLocation): Promise<FileTarget> {
  const initial = fileTarget(context, input);
  // Canonical browser paths remain usable even when an old shell cannot report cwd.
  if (!input.tabId || !context.resolveDirectory || input.path === '~' || input.path.startsWith('~/') || path.posix.isAbsolute(input.path) || path.win32.isAbsolute(input.path)) return initial;
  try {
    const cwd = await context.resolveDirectory(input.tabId), tab = context.tab(input.tabId), terminal = context.terminal?.(input.tabId);
    return fileTarget({ ...context,
      tab: id => id === input.tabId && tab ? { ...tab, cwd } : context.tab(id),
      terminal: id => id === input.tabId && terminal ? { ...terminal, cwd, directoryUrl: null } : context.terminal?.(id),
    }, input);
  } catch (error) {
    if (/^\.(?:[/\\])?$/.test(input.path)) throw new FileError('UNSUPPORTED', error instanceof Error ? error.message : 'The current directory is unavailable. Browse Home or enter an absolute folder.');
    // Old terminal links keep the documented launch/report fallback. The explicit
    // Current directory action above must never label a launch path as current.
    return initial;
  }
}
export function fileTarget(context: FileSessionContext, input: FileLocation, platform = process.platform): FileTarget {
  if (!fileLocationSchema.safeParse(input).success) throw new FileError('VALIDATION', 'Invalid file location.');
  if (input.tabId && !input.sessionId) throw new FileError('VALIDATION', 'A tab requires its session identity.');
  const session = input.sessionId ? context.session(input.sessionId) : { cwd: homedir() };
  if (!session) throw new FileError('NOT_FOUND', 'Session not found.');
  const tab = input.tabId ? context.tab(input.tabId) : undefined;
  if (input.tabId && (!tab || tab.sessionId !== input.sessionId)) throw new FileError('NOT_FOUND', 'Tab does not belong to this session.');
  const terminal = input.tabId ? context.terminal?.(input.tabId) : undefined;
  const unc = parseWslUnc(tab?.cwd ?? session.cwd);
  const distro = terminal?.root.environment === 'wsl' ? terminal.root.distro : unc?.distro ?? (tab?.terminal?.profileId.startsWith('wsl:') ? tab.terminal.profileId.slice(4) : null);
  const wsl = platform === 'win32' && !!distro;
  let cwd = terminal?.cwd ?? tab?.cwd ?? session.cwd;
  if (wsl && unc && cwd === (tab?.cwd ?? session.cwd)) cwd = unc.guestPath;
  if (terminal?.directoryUrl) {
    const url = new URL(terminal.directoryUrl);
    const localHosts = new Set(['', 'localhost', hostname().toLowerCase()]);
    // OSC 7 is process-controlled metadata, not permission to pick another host.
    if (!localHosts.has(url.hostname.toLowerCase())) throw new FileError('UNSUPPORTED', 'This terminal reports a different host. Open that machine as a Shellfox remote host to browse its files.');
    cwd = decodeURIComponent(url.pathname);
    if (platform === 'win32' && !wsl && /^\/[a-z]:\//i.test(cwd)) cwd = cwd.slice(1);
  }
  const style = platform === 'win32' && !wsl ? path.win32 : path.posix;
  let requested = input.path;
  if (requested === '~' || requested.startsWith('~/')) {
    if (wsl) throw new FileError('UNSUPPORTED', 'Use an absolute WSL path rather than ~.');
    requested = style.join(homedir(), requested.slice(2));
  }
  if (wsl) {
    const requestedUnc = parseWslUnc(requested);
    if (requestedUnc) {
      if (requestedUnc.distro.toLowerCase() !== distro!.toLowerCase()) throw new FileError('ACCESS_DENIED', 'The path belongs to another WSL distribution.');
      requested = requestedUnc.guestPath;
    }
    if (/^[a-z]:|\\/i.test(requested) || !path.posix.isAbsolute(cwd)) throw new FileError('VALIDATION', 'Expected a path in this WSL distribution.');
  }
  if (!wsl && platform === 'win32') {
    for (const value of [cwd, requested]) {
      if (/^[/\\]{2}/.test(value) || /(^|[\\/])[^\\/]*:/.test(value.replace(/^[a-z]:/i, '')) || /^[a-z]:[^\\/]/i.test(value) || /(?:^|[\\/])(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|[\\/]|$)/i.test(value)) throw new FileError('ACCESS_DENIED', 'Network, device and alternate-stream paths are not allowed.');
    }
  }
  if (!style.isAbsolute(cwd) && !style.isAbsolute(requested)) throw new FileError('UNSUPPORTED', 'The terminal working directory is not available.');
  const absolute = style.isAbsolute(requested) ? style.normalize(requested) : style.resolve(cwd, requested);
  const root = wsl ? `\\\\wsl.localhost\\${distro}` : '';
  const native = wsl ? root + absolute.replace(/\//g, '\\') : absolute;
  return {
    native,
    display: resolved => wsl ? '/' + path.win32.relative(root, resolved).replace(/\\/g, '/') : resolved,
  };
}
