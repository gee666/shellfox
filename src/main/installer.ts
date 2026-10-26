import { app } from 'electron';
import path from 'node:path';
import { mkdir } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import type { CliPort } from './platform/shellfox-cli';
import type { ExplorerPort } from './platform/explorer';
import type { NativeBackend, NativeInit } from '../shared/native-port';
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
// Installer events run before ownership acquisition. Never open the host database here.
// Providers verify ownership before replacing or removing their own integration.
export async function handleEmbeddedInstaller(event: string, explorer: ExplorerPort, cli: CliPort): Promise<void> {
  if (event === '--squirrel-obsolete') return;
  await app.whenReady();
  const removing = event === '--squirrel-uninstall';
  const folder = removing ? await explorer.set(false) : await explorer.get();
  if (!folder.ok) throw new Error('Installer integration check failed');
  if (!removing && folder.value.supported) {
    const result = await explorer.set(true);
    if (!result.ok) throw new Error('Installer integration failed');
  }
  if (!cli.readOnly) {
    const command = removing ? await cli.set(false) : await cli.get();
    if (!command.ok) throw new Error('Shellfox CLI installation check failed');
    if (!removing && command.value.supported && !(await cli.set(true)).ok) throw new Error('Shellfox CLI installation failed');
  }
  await shortcut(removing);
}
export async function handleInstaller(event: string, backend: NativeBackend, init: NativeInit): Promise<void> {
  if (event === '--squirrel-obsolete') return;
  await app.whenReady();
  await mkdir(init.userDataDir, { recursive: true });
  const unsubscribe = backend.subscribe(() => {});
  try {
    const initialized = resultSchema(probeSchema).parse(await backend.initialize(init));
    if (!initialized.ok) throw new Error('Installer native initialization failed');
    const removing = event === '--squirrel-uninstall';
    // Native removal checks the app ownership marker. No registry key is deleted here.
    const current = removing ? undefined : resultSchema(explorerSchema).parse(await backend.getExplorerIntegration());
    if (current && !current.ok) throw new Error('Installer integration check failed');
    if (removing || current?.ok && current.value.supported) {
      const result = resultSchema(explorerSchema).parse(await backend.setExplorerIntegration({ installed: !removing, executablePath: process.execPath }));
      if (!result.ok) throw new Error('Installer integration failed');
    }
    await shortcut(removing);
  } finally { unsubscribe(); await backend.dispose(); }
}
