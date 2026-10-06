import { app } from 'electron';
import path from 'node:path';
import { mkdir } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import type { CliPort } from './platform/shellfox-cli';
import type { ExplorerPort } from './platform/explorer';
import type { NativeBackend, NativeInit } from '../shared/native-port';
import { Repository } from './repository';
import { explorerSchema, probeSchema, resultSchema } from '../shared/schemas';
export function installerEvent(args: string[]): string | null {
  return args.find(a => ['--squirrel-install', '--squirrel-updated', '--squirrel-uninstall', '--squirrel-obsolete'].includes(a)) ?? null;
}
async function shortcut(remove: boolean): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    const update = path.resolve(path.dirname(process.execPath), '..', 'Update.exe');
    const child = spawn(update, [remove ? '--removeShortcut' : '--createShortcut', path.basename(process.execPath)], { shell: false, windowsHide: true, stdio: 'ignore' });
    const timer = setTimeout(() => { child.kill(); reject(new Error('Installer timed out')); }, 10000);
    child.once('error', () => { clearTimeout(timer); reject(new Error('Installer shortcut failed')); });
    child.once('exit', code => { clearTimeout(timer); code === 0 ? resolve() : reject(new Error('Installer shortcut failed')); });
  });
}
// Reapply the saved opt-in on update; remove only app-owned verbs on uninstall.
// Like the legacy installer, this runs before the single-instance lock.
export async function handleEmbeddedInstaller(event: string, explorer: ExplorerPort, cli: CliPort): Promise<void> {
  if (event === '--squirrel-obsolete') return;
  await app.whenReady();
  await mkdir(app.getPath('userData'), { recursive: true });
  const repository = new Repository(path.join(app.getPath('userData'), 'manager.sqlite3'));
  try {
    const removing = event === '--squirrel-uninstall';
    {
      // Always run ownership-checked legacy cleanup, even when the saved opt-in is off.
      const result = await explorer.set(!removing && repository.explorerPreference());
      if (!result.ok) throw new Error('Installer integration failed');
      if (removing) repository.saveExplorerPreference(false);
    }
    const installCli = !removing && (repository.cliPreference() ?? true);
    const command = await cli.set(installCli);
    if (!command.ok) throw new Error('Shellfox CLI installation failed');
    repository.saveCliPreference(installCli);
    await shortcut(removing);
  } finally { repository.close(); }
}
export async function handleInstaller(event: string, backend: NativeBackend, init: NativeInit): Promise<void> {
  if (event === '--squirrel-obsolete') return;
  await app.whenReady();
  await mkdir(init.userDataDir, { recursive: true });
  const repository = new Repository(path.join(init.userDataDir, 'manager.sqlite3'));
  const unsubscribe = backend.subscribe(() => {});
  try {
    const initialized = resultSchema(probeSchema).parse(await backend.initialize(init));
    if (!initialized.ok) throw new Error('Installer native initialization failed');
    const removing = event === '--squirrel-uninstall';
    // Native removal checks the app ownership marker. No registry key is deleted here.
    if (removing || repository.explorerPreference()) {
      const result = resultSchema(explorerSchema).parse(await backend.setExplorerIntegration({ installed: !removing, executablePath: process.execPath }));
      if (!result.ok) throw new Error('Installer integration failed');
      if (removing) repository.saveExplorerPreference(false);
    }
    await shortcut(removing);
  } finally { unsubscribe(); await backend.dispose(); repository.close(); }
}
