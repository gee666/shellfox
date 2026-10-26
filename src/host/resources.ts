import path from 'node:path';
import { hostname } from 'node:os';
import type { HostRuntimeOptions, HostResources } from './runtime';
import { HeadlessKeepAwake } from './keep-awake';

import { hostResourcePaths } from './paths';
import { bundledHostCli } from './cli-integration';
import { createBoundHostCli } from './cli-binding';
export { hostResourcePaths } from './paths';
/** Loaded only after ownership. None of these modules imports Electron. */
export async function createHostResources(options: HostRuntimeOptions): Promise<HostResources> {
  const paths = hostResourcePaths(options);
  const [{ Repository }, { EmbeddedSessionService }, { PtyBackend }, { ProcessTracker, createSystemSnapshotProvider }, { RemoteService }, { RemoteHelper }, { RemoteStore }, { SshProfileStore }, { startControlServer }] = await Promise.all([
    import('../main/repository'), import('../main/terminal/service'), import('../main/terminal/backend'), import('../main/terminal/tracking'),
    import('../main/remote/service'), import('../main/remote/helper'), import('../main/remote/storage'), import('../main/ssh/storage'), import('../main/control-server'),
  ]);
  const bundledCli = await bundledHostCli();
  const cli = options.cliBinding ? await createBoundHostCli(options.userData, options.cliBinding, { ...paths.helpers, runtimePath: bundledCli?.binDir }) : bundledCli;
  options.assertOwnership?.();
  const repository = new Repository(path.join(options.userData, 'manager.sqlite3'));
  const backend = new PtyBackend({ helperOptions: paths.helpers, allowAdministrator: options.mode === 'desktop', hostEnvironment: { userData: options.userData, runtimePath: bundledCli?.binDir, resourcesPath: paths.helpers.resourcesPath, headless: options.mode !== 'desktop' } });
  const service = new EmbeddedSessionService(repository, backend, emit => new ProcessTracker(emit, createSystemSnapshotProvider(paths.helpers)), undefined, cli, paths.helpers);
  const remoteHostname = hostname().replace(/[^a-zA-Z0-9-]/g, '-').replace(/^-+|-+$/g, '').slice(0, 63).replace(/-+$/g, '') || 'shellfox';
  const remote = new RemoteService(options.userData, new RemoteStore(options.userData, remoteHostname), new RemoteHelper(options.helperPath ?? paths.tailscale), service, remoteHostname);
  const keepAwake = new HeadlessKeepAwake();
  return { service, remote, keepAwake, ssh: new SshProfileStore(options.userData), closeRepository: () => repository.close(), initialize: async () => { await keepAwake.initialize(); await service.initialize(); await remote.initialize(); }, startLegacyControl: () => startControlServer(options.userData, service, keepAwake, options.assertOwnership) };
}
