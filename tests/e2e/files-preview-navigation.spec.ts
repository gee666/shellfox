import { _electron, expect, test } from '@playwright/test';
import { build } from 'esbuild';
import { rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { scratch } from '../fixtures/electron';
import { desktopPreviewUrl } from '../../src/shared/file-preview-url';

test('a preview cannot replace itself or open an outbound destination', async () => {
  const directory = await scratch('preview-navigation');
  const requests: string[] = [];
  const listener = createServer((request, response) => { requests.push(request.url ?? ''); response.end('loopback'); });
  try {
    await new Promise<void>((resolve, reject) => {
      listener.once('error', reject);
      listener.listen(0, '127.0.0.1', resolve);
    });
    const address = listener.address();
    if (!address || typeof address === 'string') throw new Error('Expected a loopback port.');
    const url = `http://127.0.0.1:${address.port}`;
    expect(await (await fetch(`${url}/ready`, { signal: AbortSignal.timeout(4000) })).text()).toBe('loopback');
    expect(requests.splice(0)).toEqual(['/ready']);
    const alternate = desktopPreviewUrl('<p>Unexpected replacement</p>');
    const document = `<!doctype html><p id="preview">Retained preview</p>
      <a href="${alternate}">Try preview replacement</a>
      <a id="link" href="${url}/link">Try navigation</a>
      <script>document.getElementById('link').addEventListener('click', () => {
        document.getElementById('preview').textContent = 'Attempted navigation';
        window.open('${url}/popup'); location.href = '${url}/frame';
      });</script>`;
    await writeFile(path.join(directory, 'preview.html'),
      `<iframe title="HTML preview" sandbox="allow-scripts" src="${desktopPreviewUrl(document)}"></iframe>`);
    const main = path.join(directory, 'main.cjs');
    await build({ entryPoints: ['tests/fixtures/preview-security-main.ts'], bundle: true,
      platform: 'node', format: 'cjs', external: ['electron'], outfile: main });
    const app = await _electron.launch({ args: [main], timeout: 20_000,
      env: { ...process.env, TMP: directory, TEMP: directory, TMPDIR: directory } });
    try {
      const page = await app.firstWindow({ timeout: 20_000 });
      const preview = page.frameLocator('iframe[title="HTML preview"]');
      await expect(preview.getByText('Retained preview')).toBeVisible();
      await preview.getByRole('link', { name: 'Try preview replacement' }).click();
      await expect(preview.getByText('Retained preview')).toBeVisible();
      const workspaceUrl = page.url();
      await preview.getByRole('link', { name: 'Try navigation', exact: true }).click();
      await expect(page.getByTitle('HTML preview')).toBeVisible();
      expect(page.url()).toBe(workspaceUrl);
      await delay(500);
      expect(requests).toEqual([]);
      expect(app.windows()).toHaveLength(1);
    } finally {
      await app.close();
    }
  } finally {
    listener.closeAllConnections();
    if (listener.listening) await new Promise<void>(resolve => listener.close(() => resolve()));
    await rm(directory, { recursive: true, force: true, maxRetries: 5 }).catch(() => undefined);
  }
});
