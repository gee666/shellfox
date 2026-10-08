import { readdir, stat } from 'node:fs/promises';
import { homedir } from 'node:os';
import path from 'node:path';
import { failure, success, type Result } from '../shared/contracts';
import { DIRECTORY_COMPLETION_BYTES, DIRECTORY_COMPLETION_LIMIT, directoryPromptPathSchema, pathSchema } from '../shared/schemas';
import { validateDirectory } from './directory';

class PromptPathError extends Error {}

function unquote(input: string): string {
  if (!directoryPromptPathSchema.safeParse(input).success) throw new PromptPathError('Path must contain at most 32767 characters and no control characters.');
  const trimmed = input.trim();
  // Trim only outside enclosing quotes. Unquoted spaces are part of the filename.
  if (trimmed.length >= 2 && (trimmed[0] === '"' || trimmed[0] === "'") && trimmed.at(-1) === trimmed[0]) return trimmed.slice(1, -1);
  return input;
}

function promptPath(value: string, home: string, platform: NodeJS.Platform): string {
  const windows = platform === 'win32', paths = windows ? path.win32 : path.posix;
  if (windows) {
    if (/^[a-z]:($|[^\\/])/i.test(value)) throw new PromptPathError('Use an absolute drive path, such as C:\\folder, not a drive-relative path.');
    if (/^[\\/]{2}[?.][\\/]/.test(value)) throw new PromptPathError('Windows device paths are not supported.');
    if (/^[\\/]{2}/.test(value) && !/^[\\/]{2}[^\\/]+[\\/][^\\/]+/.test(value)) throw new PromptPathError('UNC paths must include a server and share.');
  } else if (/^[a-z]:/i.test(value) || /^\\/.test(value) || /^\/{2}/.test(value)) {
    throw new PromptPathError('Windows drive and UNC paths require Windows.');
  }
  if (value === '~') return paths.resolve(home);
  if (value.startsWith('~/') || windows && value.startsWith('~\\')) return paths.resolve(home + paths.sep + value.slice(2));
  return paths.resolve(home, value);
}

/** Native filesystem syntax only. Windows drive-relative paths never use process.cwd(). */
export function directoryPromptPath(input: string, home = homedir(), platform: NodeJS.Platform = process.platform): string {
  return promptPath(unquote(input), home, platform);
}

/** Keep the typed prefix separate from the absolute directory used for filesystem access. */
export function directoryCompletionTarget(input: string, home = homedir(), platform: NodeJS.Platform = process.platform): { directory: string; prefix: string; fragment: string; separator: string } {
  const value = unquote(input), windows = platform === 'win32', paths = windows ? path.win32 : path.posix;
  promptPath(value, home, platform); // Reject invalid drive/UNC syntax before splitting it.
  const last = windows ? Math.max(value.lastIndexOf('/'), value.lastIndexOf('\\')) : value.lastIndexOf('/');
  const separator = last < 0 ? paths.sep : value[last];
  if (value === '~' || value === '.' || value === '..' || paths.isAbsolute(value) && paths.parse(value).root === value) {
    return { directory: promptPath(value, home, platform), prefix: value.endsWith(separator) ? value : value + separator, fragment: '', separator };
  }
  const prefix = value.slice(0, last + 1), fragment = value.slice(last + 1);
  return { directory: promptPath(prefix, home, platform), prefix, fragment, separator };
}

function pathFailure(error: unknown, completion: boolean): Result<never> {
  if (error instanceof PromptPathError) return failure('VALIDATION', error.message);
  const code = (error as NodeJS.ErrnoException | null)?.code;
  const action = completion ? 'Cannot complete path' : 'Cannot resolve directory';
  if (code === 'ENOENT' || code === 'ENOTDIR') return failure('NOT_FOUND', action + ': directory does not exist or is not a directory.');
  if (code === 'EACCES' || code === 'EPERM') return failure('VALIDATION', action + ': directory is not accessible.');
  return completion
    ? failure('INTERNAL', 'Could not read directory completions.', true)
    : failure('VALIDATION', 'Cannot resolve directory: choose an accessible local or WSL directory.');
}

export function getHomeDirectory(): Result<{ cwd: string }> {
  try {
    const cwd = path.normalize(homedir());
    return pathSchema.safeParse(cwd).success ? success({ cwd }) : failure('VALIDATION', 'Home directory is not a supported absolute path.');
  } catch { return failure('INTERNAL', 'Could not determine the home directory.'); }
}

export async function resolveDirectory(input: { path: string }): Promise<Result<{ cwd: string }>> {
  try { return success({ cwd: await validateDirectory(directoryPromptPath(input.path)) }); }
  catch (error) { return pathFailure(error, false); }
}

export async function completeDirectory(input: { path: string }): Promise<Result<{ matches: string[] }>> {
  try {
    const { directory, prefix, fragment, separator } = directoryCompletionTarget(input.path);
    const windows = process.platform === 'win32';
    const needle = windows ? fragment.toLowerCase() : fragment;
    const entries = (await readdir(directory, { withFileTypes: true }))
      .filter(entry => (windows ? entry.name.toLowerCase() : entry.name).startsWith(needle))
      .sort((a, b) => a.name < b.name ? -1 : a.name > b.name ? 1 : 0);
    const matches: string[] = [];
    let bytes = 0;
    for (const entry of entries) {
      let isDirectory = entry.isDirectory();
      if (entry.isSymbolicLink()) {
        try { isDirectory = (await stat(path.join(directory, entry.name))).isDirectory(); }
        catch (error) {
          // A dangling/disappearing link is not a completion. Other I/O failures are reported.
          if (!['ENOENT', 'ENOTDIR'].includes((error as NodeJS.ErrnoException).code ?? '')) throw error;
        }
      }
      if (!isDirectory) continue;
      const match = prefix + entry.name + separator;
      if (!directoryPromptPathSchema.safeParse(match).success) continue;
      const size = Buffer.byteLength(match, 'utf8');
      if (bytes + size > DIRECTORY_COMPLETION_BYTES) break;
      matches.push(match); bytes += size;
      if (matches.length === DIRECTORY_COMPLETION_LIMIT) break;
    }
    return success({ matches });
  } catch (error) { return pathFailure(error, true); }
}
