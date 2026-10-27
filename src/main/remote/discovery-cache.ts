import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { z } from 'zod';
import { remotePeerIdSchema } from '../../shared/remote-schemas';
const cacheSchema = z.object({ version: z.literal(1), ids: z.array(remotePeerIdSchema).max(1000) }).strict();

/** Only confirmed stable IDs, never addresses, credentials or approval grants. */
export class DiscoveryCache {
  private queue: Promise<void> = Promise.resolve();
  constructor(private readonly directory: string) {}
  async load(): Promise<string[]> {
    try {
      const data = await readFile(path.join(this.directory, 'discovered-hosts.json'), 'utf8');
      if (Buffer.byteLength(data) > 256 * 1024) return [];
      return [...new Set(cacheSchema.parse(JSON.parse(data)).ids)];
    } catch { return []; }
  }
  save(ids: string[]): Promise<void> {
    const data = JSON.stringify(cacheSchema.parse({ version: 1, ids: [...new Set(ids)].sort() }));
    const next = this.queue.catch(() => {}).then(async () => {
      await mkdir(this.directory, { recursive: true, mode: 0o700 });
      const file = path.join(this.directory, 'discovered-hosts.json');
      await writeFile(file + '.new', data, { mode: 0o600 });
      await rename(file + '.new', file);
    });
    this.queue = next;
    return next;
  }
}
