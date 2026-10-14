import path from 'node:path';
import { mkdir, open, readFile, rename, stat, unlink } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';

export type ScriptLocation = { path: string; distro?: string };
export type CustomCommand = ScriptLocation & { name: string };
const namePattern = /^[a-zA-Z][a-zA-Z0-9_-]{0,63}$/;
export function commandKey(name: string): string { return name.toLowerCase(); }
export function validateCommandName(name: string): void {
  if (!namePattern.test(name)) throw new Error('Command names must start with a letter and contain only letters, digits, _ or -, up to 64 characters.');
}
function validateRegistry(value: unknown): CustomCommand[] {
  const registry = value as { version?: unknown; commands?: unknown } | null;
  if (!registry || registry.version !== 1 || !Array.isArray(registry.commands)) throw new Error('Invalid custom command registry.');
  const names = new Set<string>();
  return registry.commands.map((entry: unknown) => {
    const command = entry as CustomCommand | null;
    if (!command || typeof command.name !== 'string' || !namePattern.test(command.name) ||
      typeof command.path !== 'string' || /[\0\r\n]/.test(command.path) ||
      !(path.posix.isAbsolute(command.path) || path.win32.isAbsolute(command.path)) ||
      (command.distro !== undefined && (typeof command.distro !== 'string' || !command.distro || /[\0\r\n]/.test(command.distro))) ||
      (command.distro !== undefined && !path.posix.isAbsolute(command.path)) || names.has(commandKey(command.name))) {
      throw new Error('Invalid custom command registry.');
    }
    names.add(commandKey(command.name));
    return { name: command.name, path: command.path, ...(command.distro ? { distro: command.distro } : {}) };
  });
}

export class CustomCommandStore {
  readonly file: string;
  constructor(readonly userData: string) {
    this.file = path.join(userData, 'cli-commands.json');
  }
  async read(): Promise<CustomCommand[]> {
    try { return validateRegistry(JSON.parse(await readFile(this.file, 'utf8'))); }
    catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT') return []; throw error; }
  }
  private async mutate(change: (commands: CustomCommand[]) => void): Promise<void> {
    await mkdir(this.userData, { recursive: true });
    const lock = this.file + '.lock';
    let handle;
    for (let attempt = 0; attempt < 100; attempt++) {
      try { handle = await open(lock, 'wx', 0o600); break; }
      catch (error) {
        if ((error as NodeJS.ErrnoException).code !== 'EEXIST') throw error;
        await new Promise(resolve => setTimeout(resolve, 20));
      }
    }
    if (!handle) throw new Error('Custom command registry is locked. Retry, or remove cli-commands.json.lock after checking no registration is running.');
    const next = this.file + '.' + randomUUID() + '.new';
    try {
      const commands = await this.read();
      change(commands);
      const output = await open(next, 'wx', 0o600);
      try { await output.writeFile(JSON.stringify({ version: 1, commands }, null, 2) + '\n'); await output.sync(); }
      finally { await output.close(); }
      await rename(next, this.file);
    } finally {
      await unlink(next).catch(error => { if (error.code !== 'ENOENT') throw error; });
      await handle.close();
      await unlink(lock);
    }
  }
  async register(name: string, location: ScriptLocation): Promise<void> {
    validateCommandName(name);
    // Reuse the on-disk validator so public callers cannot persist invalid paths.
    validateRegistry({ version: 1, commands: [{ name, ...location }] });
    await this.mutate(commands => {
      if (commands.some(command => commandKey(command.name) === commandKey(name))) throw new Error('Command already registered: ' + name);
      commands.push({ name, ...location });
    });
  }
  async unregister(name: string): Promise<void> {
    await this.mutate(commands => {
      const index = commands.findIndex(command => commandKey(command.name) === commandKey(name));
      if (index < 0) throw new Error('No custom command named ' + name);
      commands.splice(index, 1);
    });
  }
  async rename(oldName: string, newName: string): Promise<void> {
    validateCommandName(newName);
    await this.mutate(commands => {
      const command = commands.find(command => commandKey(command.name) === commandKey(oldName));
      if (!command) throw new Error('No custom command named ' + oldName);
      if (commands.some(other => other !== command && commandKey(other.name) === commandKey(newName))) throw new Error('Command already registered: ' + newName);
      command.name = newName;
    });
  }
}

export async function requireScriptFile(file: string): Promise<void> {
  if (!(await stat(file)).isFile()) throw new Error('Script path must refer to a file.');
}
