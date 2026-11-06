/// <reference path="../../src/main/update/assets.d.ts" />
// Explicit QA-only Windows process. Never imported by production desktop/mobile entries.
import { app } from 'electron';
import { mkdir, writeFile, readFile, stat } from 'node:fs/promises';
import path from 'node:path';
import { randomBytes } from 'node:crypto';
import { Repository } from '../../src/main/repository';
import { EmbeddedSessionService } from '../../src/main/terminal/service';
import { PtyBackend } from '../../src/main/terminal/backend';
import { RemoteHost } from '../../src/main/remote/host';
import { RemoteApprovals } from '../../src/main/remote/approvals';
import { populateRemoteHost, remoteHostSnapshot } from './remote-host-fixtures';
import { startQaRemoteRelay, type QaRequest, type QaRemoteConfiguration } from './remote-host-relay';

async function main(): Promise<void> {
  if (process.env.SHELLFOX_REAL_FILES_QA !== '1' || process.platform !== 'win32') throw new Error('This explicit isolated QA entry requires Windows and SHELLFOX_REAL_FILES_QA=1.');
  const args = process.argv.slice(2), argument = (flag: string) => { const index = args.indexOf(flag); return index < 0 ? undefined : args[index + 1]; };
  const runDirectory = path.resolve(argument('--run-dir') ?? path.join('tmp', 'mobile-real-remote-host', `${Date.now()}-${process.pid}`));
  const relative = path.relative(path.resolve('tmp'), runDirectory);
  if (!relative || relative === '..' || relative.startsWith('..' + path.sep) || path.isAbsolute(relative)) throw new Error('QA artifacts must live under the project tmp directory.');
  const userData = path.join(runDirectory, 'host-data'), root = path.join(runDirectory, 'Windows-host-files');
  await mkdir(userData, { recursive: true }); await populateRemoteHost(root);
  app.setPath('userData', path.join(runDirectory, 'electron-data'));
  await app.whenReady();
  const repository = new Repository(path.join(userData, 'manager.sqlite3'));
  // No interactive processes or session fixtures are required for the real filesystem browser.
  const backend = new PtyBackend({ discover: async () => ({ profiles: [], defaultProfileId: null, lifetime: 'app-owned', shellSurvival: false }) });
  const service = new EmbeddedSessionService(repository, backend);
  await service.files.initialize(userData); await service.initialize();
  const approved = new Set<string>(), audit: QaRequest[] = [];
  const approvals = new RemoteApprovals(id => approved.has(id), () => {});
  const configuration: QaRemoteConfiguration = {
    protocol: 1, hostId: 'windows-real-host-qa', clientPeer: { id: 'android-real-files-qa', hostname: 'Android emulator QA' },
    hmacSecret: randomBytes(32).toString('hex'), root,
    files: ['code.ts', 'README.md', 'index.html', 'local.js', 'style.css', 'image.png', 'transfer.bin', 'host-subfolder/nested.txt'],
  };
  const host = new RemoteHost(service, configuration.hmacSecret, id => approved.has(id),
    (peer, clientId) => { approvals.authorize(peer, clientId); }, (peer, clientId) => approvals.authorize(peer, clientId),
    (peer, clientId) => approvals.withdraw(peer.id, clientId), { root: path.join(userData, 'unused-terminal-inbox') });
  const backendUrl = await host.listen(), controlToken = randomBytes(32).toString('hex');
  let stopped = false;
  const status = async () => {
    const code = path.join(root, 'code.ts'), size = (await stat(code)).size;
    return { hostPid: process.pid, hostPlatform: process.platform, root, approved: [...approved], pending: [...approvals.pending.values()],
      files: await remoteHostSnapshot(root), codeText: size <= 65536 ? await readFile(code, 'utf8') : '<larger than QA text status limit>', requests: [...audit] };
  };
  const relay = await startQaRemoteRelay({
    port: 41973, backendUrl, configuration, controlToken, status,
    record: request => { audit.push(request); if (audit.length > 8192) audit.shift(); },
    control: async action => {
      const peerId = configuration.clientPeer.id;
      if (action === 'approve') { approved.add(peerId); approvals.approve(peerId); }
      else if (action === 'revoke') { approved.delete(peerId); approvals.deny(peerId); host.revoke(peerId); }
      else setTimeout(() => { void stop(); }, 100);
    },
  });
  const stop = async () => {
    if (stopped) return; stopped = true; clearTimeout(deadline);
    await relay.close(); await host.dispose(); await service.dispose(); repository.close();
    await writeFile(path.join(runDirectory, 'host-final-state.json'), JSON.stringify(await remoteHostSnapshot(root), null, 2));
    app.quit();
  };
  const deadline = setTimeout(() => { void stop(); }, 30 * 60_000);
  process.once('SIGINT', () => { void stop(); }); process.once('SIGTERM', () => { void stop(); });
  const ready = { hostPid: process.pid, hostPlatform: process.platform, baseUrl: 'http://127.0.0.1:41973', androidBaseUrl: 'http://10.0.2.2:41973',
    root, controlToken, configuration, initialFiles: await remoteHostSnapshot(root), coverage: 'Real Windows disk + production RemoteClient/RemoteHost HMAC approvals/chunk API; QA-only loopback transport bypasses tsnet enrollment and dialing.' };
  await writeFile(path.join(runDirectory, 'ready.json'), JSON.stringify(ready, null, 2), { mode: 0o600, flag: 'wx' });
  console.log('REAL_REMOTE_HOST_READY ' + JSON.stringify({ runDirectory, root, hostPid: process.pid }));
}
void main().catch(error => { console.error(error instanceof Error ? error.stack : String(error)); app.exit(1); });
