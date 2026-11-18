import { randomUUID } from 'node:crypto';
import { mkdir, mkdtemp, readFile } from 'node:fs/promises';
import path from 'node:path';
import { it } from 'vitest';
import { assertControlPermissions } from './control-permission.assertions';
import { controlEndpointPath } from './control-protocol';
import { startControlServer } from './control-server';
import { PtyBackend } from './terminal/backend';
import { EmbeddedSessionService } from './terminal/service';
import { factoryFixture, MemoryRepository } from './terminal/test-fixtures';

// HOLD. This counterpart preserves the real Windows ACL assertion, without the unit permission model.
it.runIf(process.platform === 'win32')('publishes private endpoint credentials and refuses unsafe permissions', async () => {
  await mkdir('tmp', { recursive: true });
  const directory = await mkdtemp(path.resolve('tmp/control-acl-test-'));
  const service = new EmbeddedSessionService(new MemoryRepository(), new PtyBackend(factoryFixture().options), () => ({ setWatch() {}, dispose() {} }));
  await service.initialize();
  await service.createSession({ cwd: '/work', requestId: randomUUID() });
  const server = await startControlServer(directory, service, { active: false, set: active => active });
  const endpoint: unknown = JSON.parse(await readFile(controlEndpointPath(directory), 'utf8'));
  await assertControlPermissions(directory, endpoint);
  await server.dispose();
  await service.dispose();
  // The future owner, not this fixture, must decide whether the root can be removed.
}, 15000);
