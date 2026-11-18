import { mkdtemp, mkdir, rm } from 'node:fs/promises';
import path from 'node:path';
import { vi } from 'vitest';
import { PtyBackend } from '../main/terminal/backend';
import { EmbeddedSessionService } from '../main/terminal/service';
import { factoryFixture, MemoryRepository } from '../main/terminal/test-fixtures';
import { connectHostClient } from './client';
import { startHostRpc } from './rpc';

export function rpcTestFixture(cleanup: Array<() => Promise<unknown>>, register?: (root: string) => void) {
  async function directory() {
    await mkdir('tmp', { recursive: true });
    const dir = await mkdtemp(path.resolve('tmp/host-test-'));
    cleanup.push(() => rm(dir, { recursive: true, force: true }));
    register?.(dir);
    return dir;
  }
  async function fixture() {
    const userData = await directory(), pty = factoryFixture(), repository = new MemoryRepository();
    const service = new EmbeddedSessionService(repository, new PtyBackend(pty.options), () => ({ setWatch() {}, dispose() {} }));
    await service.initialize(); cleanup.push(() => service.dispose());
    const shutdown = vi.fn(async () => {});
    const rpc = await startHostRpc({ userData, service, shutdown, mode: 'headless' }); cleanup.push(() => rpc.close());
    const connect = async () => { const client = await connectHostClient(userData); cleanup.push(() => client.close()); return client; };
    return { userData, service, pty, repository, shutdown, rpc, connect };
  }
  return { directory, fixture };
}
