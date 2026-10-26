import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { z } from 'zod';
import { remoteApprovalSchema, remoteHostnameSchema, remoteNetworkPeerSchema } from '../../shared/remote-schemas';
const storageSchema = z.object({ version: z.literal(1), enabled: z.boolean(), hostname: remoteHostnameSchema, approvals: z.array(remoteApprovalSchema).max(1000), peers: z.array(remoteNetworkPeerSchema).max(1000) }).strict();
export type RemoteStoredState = z.infer<typeof storageSchema>;
/** No keys, terminal data, session environment, or snapshots belong in this file. */
export class RemoteStore {
  private queue: Promise<void> = Promise.resolve();
  constructor(private readonly directory: string, private readonly hostname: string) {}
  async load(): Promise<RemoteStoredState> {
    try { return storageSchema.parse(JSON.parse(await readFile(path.join(this.directory, 'remote.json'), 'utf8'))); }
    catch (error) {
      if (!(error instanceof Error && 'code' in error && error.code === 'ENOENT')) throw new Error('Remote preferences could not be read.');
      return { version: 1, enabled: false, hostname: this.hostname, approvals: [], peers: [] };
    }
  }
  save(state: RemoteStoredState): Promise<void> {
    const data = JSON.stringify(storageSchema.parse(state));
    const next = this.queue.catch(() => {}).then(async () => {
      await mkdir(this.directory, { recursive: true, mode: 0o700 });
      const file = path.join(this.directory, 'remote.json');
      await writeFile(file + '.new', data, { mode: 0o600 });
      await rename(file + '.new', file);
    });
    this.queue = next;
    return next;
  }
}
