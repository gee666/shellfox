import type { TrackingRoot } from './tracking';
import { execute, type Execute } from './profiles';
import { UNIX_TREE_CLOSE_SCRIPT, compactOwnedIdentities, type UnixIdentity } from './unix-close';
import path from 'node:path';
export const GUEST_CLOSE_SCRIPT = UNIX_TREE_CLOSE_SCRIPT;
/** Hold pidfds for the guest shell and owned descendants. Never kill wsl.exe before tree confirmation. */
export async function closeGuest(root: TrackingRoot, identity: UnixIdentity | null, run: Execute = execute, known: UnixIdentity[] = []): Promise<void> {
  if (!root.distro || !identity || !Number.isSafeInteger(identity.pid) || identity.pid < 1 || !/^[0-9a-fA-F-]{32,36}:\d+$/.test(identity.birth)) throw new Error('Authenticated guest root identity unavailable.');
  const wsl = path.win32.join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'wsl.exe');
  await run(wsl, ['--distribution', root.distro, '--exec', 'python3', '-c', GUEST_CLOSE_SCRIPT, String(identity.pid), identity.birth, root.marker, JSON.stringify(compactOwnedIdentities(identity, known))]);
}
