import path from 'node:path';
import { statSync } from 'node:fs';

/** Resolve a Windows executable without ever interpreting shell metacharacters. */
export function resolveAutomationExecutable(executable: string, cwd: string, env: Record<string, string>): string {
  const value = (name: string) => Object.entries(env).find(([key]) => key.toLowerCase() === name.toLowerCase())?.[1];
  const extensions = (value('PATHEXT') ?? '.COM;.EXE').split(';').map(extension => extension.toLowerCase()).filter(extension => extension === '.exe' || extension === '.com');
  const suffixes = path.win32.extname(executable) ? [''] : ['', ...extensions];
  const explicit = path.win32.isAbsolute(executable) || /[\\/]/.test(executable);
  const directories = explicit ? [''] : [cwd, ...(value('PATH') ?? '').split(';').map(directory => directory.replace(/^"(.*)"$/, '$1')).filter(Boolean)];
  for (const directory of directories) for (const suffix of suffixes) {
    const file = path.win32.resolve(cwd, directory, executable + suffix);
    if (!/\.(?:exe|com)$/i.test(file)) continue;
    try { if (statSync(file).isFile()) return file; } catch { /* Continue searching the caller's PATH. */ }
  }
  throw new Error('Automation executable not found. Use an executable on PATH; invoke .cmd/.bat scripts explicitly through cmd.exe.');
}
