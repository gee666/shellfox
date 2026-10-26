import { _electron, type ElectronApplication, type Page } from '@playwright/test';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdir } from 'node:fs/promises';
import path from 'node:path';
import type { RemoteState } from '../../src/shared/remote-contracts';
import { env, root, scratch, testMain, value } from './electron';

export async function remoteApps(label: string) {
  const directory = await scratch(label), bus = path.join(directory, 'bus'); await mkdir(bus);
  const helper = path.join(directory, process.platform === 'win32' ? 'test-remote-helper.exe' : 'test-remote-helper');
  const goWork = path.join(directory, 'go-work'); await mkdir(goWork);
  await promisify(execFile)(process.env.SHELLFOX_GO ?? 'go', ['build', '-o', helper, './tests/fixtures/remote-helper/main.go'], {
    cwd: root, timeout: 120_000,
    env: { ...env(), CGO_ENABLED: '0', GOTOOLCHAIN: 'local', GOENV: 'off', GOWORK: 'off', GOFLAGS: '',
      GOOS: process.platform === 'win32' ? 'windows' : process.platform, GOARCH: process.arch === 'x64' ? 'amd64' : 'arm64',
      GOCACHE: path.join(root, 'tmp/go-cache'), GOMODCACHE: path.join(root, 'tmp/go-mod'), GOTMPDIR: goWork },
  });
  const applications = new Set<ElectronApplication>();
  async function open(name: string) {
    const app = await _electron.launch({ cwd: root, args: [testMain, '--test-user-data', path.join(directory, name), '--test-backend', 'real'], env: { ...env(), SHELLFOX_TEST_REMOTE_HELPER: helper, SHELLFOX_TEST_REMOTE_BUS: bus, SHELLFOX_TEST_CONFIRM_QUIT: '1' }, timeout: 30_000 });
    applications.add(app); const page = await app.firstWindow();
    await page.waitForFunction(() => !!window.shellfoxRemote);
    return { app, page };
  }
  async function state(page: Page): Promise<RemoteState> { return value(await page.evaluate(() => window.shellfoxRemote!.getState())); }
  async function close() {
    for (const app of applications) {
      const page = await app.firstWindow().catch(() => null);
      await page?.evaluate(async () => {
        const snapshot = await window.shellfox.getSnapshot();
        if (snapshot.ok) for (const session of snapshot.value.sessions) for (const tab of session.tabs) {
          if (tab.generation && tab.lifecycle !== 'closed') await window.shellfox.closeTab!({ tabId: tab.id, generation: tab.generation });
        }
      }).catch(() => {});
      await app.close().catch(() => {});
    }
  }
  return { directory, open, state, close, applications };
}
