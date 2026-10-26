import { expect, test } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { value } from '../fixtures/electron';
import { remoteApps } from '../fixtures/remote-apps';

// Only the network transport is simulated. Electron, preload, session storage,
// PTYs, host approval, delivery, and terminal rendering use production code.
test('remote approval, client quit and reconnect preserve the host terminal', async () => {
  test.setTimeout(180_000);
  const { directory, open, state, applications } = await remoteApps('remote-connections');
  let host: Awaited<ReturnType<typeof open>> | undefined;
  try {
    host = await open('host');
    let client = await open('client');
    value(await host.page.evaluate(() => window.shellfoxRemote!.configure({ enabled: true, hostname: 'workstation' })));
    value(await client.page.evaluate(() => window.shellfoxRemote!.configure({ enabled: true, hostname: 'laptop' })));
    await expect.poll(async () => (await state(client.page)).peers.some(peer => peer.id === 'workstation')).toBe(true);
    value(await client.page.evaluate(() => window.shellfoxRemote!.connect({ peerId: 'workstation' })));
    await expect.poll(async () => (await state(host!.page)).pending.map(peer => peer.id)).toContain('laptop');
    const denied = await client.page.evaluate(() => window.shellfoxRemote!.request({ peerId: 'workstation', method: 'getSnapshot', payload: {} }));
    expect(denied.ok).toBe(false);
    value(await host.page.evaluate(() => window.shellfoxRemote!.approve({ peerId: 'laptop' })));
    await expect.poll(async () => (await state(client.page)).peers.find(peer => peer.id === 'workstation')?.connection).toBe('connected');
    const session = value(await client.page.evaluate(input => window.shellfoxRemote!.request({ peerId: 'workstation', method: 'createSession', payload: input }), { cwd: directory, title: 'Remote agent', requestId: randomUUID() }));
    const clientLocal = value(await client.page.evaluate(() => window.shellfox.getSnapshot()));
    expect(clientLocal.sessions.some(item => item.id === session.id)).toBe(false);
    const tab = session.tabs.find(tab => tab.lifecycle === 'open');
    expect(tab?.generation).toBeTruthy();
    if (!tab?.generation) throw new Error('Host terminal did not open');
    const identity = { tabId: tab.id, generation: tab.generation };
    await expect(host.page.getByRole('button', { name: 'Select session Remote agent' })).toBeVisible();
    await expect(client.page.getByRole('button', { name: 'Select session Remote agent' })).toBeVisible();
    await client.page.getByRole('button', { name: 'Select session Remote agent' }).click();
    await expect(client.page.getByRole('region', { name: 'Terminal viewport' })).toBeVisible();
    await expect(client.page.locator('.terminal-loading-overlay')).toHaveCount(0);
    await client.page.screenshot({ path: path.join(directory, 'remote-sidebar.png') });
    await client.page.getByRole('button', { name: 'Settings', exact: true }).click();
    await client.page.getByRole('tab', { name: 'Remote access', exact: true }).click();
    await expect(client.page.locator('.remote-settings')).toBeVisible();
    await client.page.screenshot({ path: path.join(directory, 'remote-settings.png') });
    await client.page.getByRole('button', { name: 'Close settings', exact: true }).click();
    const progress = path.join(directory, 'progress.txt');
    const command = process.platform === 'win32'
      ? `1..80 | ForEach-Object { Add-Content -LiteralPath '${progress.replaceAll("'", "''")}' -Value $_; Start-Sleep -Milliseconds 100 }\r`
      : `for i in $(seq 1 80); do printf '%s\\n' "$i" >> '${progress.replaceAll("'", "'\\''")}'; sleep 0.1; done\r`;
    value(await client.page.evaluate(input => window.shellfoxRemote!.request({ peerId: 'workstation', method: 'writeTerminal', payload: input }), { ...identity, data: command }));
    await expect.poll(async () => (await readFile(progress, 'utf8').catch(() => '')).trim().split(/\r?\n/).filter(Boolean).length).toBeGreaterThan(0);
    await client.app.close(); applications.delete(client.app);
    // It must still be running after the client exits, not finish during shutdown.
    expect((await readFile(progress, 'utf8')).trim().split(/\r?\n/).length).toBeLessThan(80);
    await expect.poll(async () => (await readFile(progress, 'utf8').catch(() => '')).trim().split(/\r?\n/).filter(Boolean).length, { timeout: 15_000 }).toBe(80);
    const local = value(await host.page.evaluate(() => window.shellfox.getSnapshot()));
    expect(local.sessions.find(item => item.id === session.id)?.tabs.find(item => item.id === tab.id)?.generation).toBe(tab.generation);
    client = await open('client');
    await expect.poll(async () => (await state(client.page)).status).toBe('online');
    value(await client.page.evaluate(() => window.shellfoxRemote!.connect({ peerId: 'workstation' })));
    await expect.poll(async () => (await state(client.page)).peers.find(peer => peer.id === 'workstation')?.connection).toBe('connected');
    const restored = value(await client.page.evaluate(input => window.shellfoxRemote!.request({ peerId: 'workstation', method: 'attachTerminal', payload: input }), identity));
    expect(restored.state).toBe('open');
    expect(restored.generation).toBe(tab.generation);
    const resumedFile = path.join(directory, 'reconnected.txt');
    const resumedCommand = process.platform === 'win32'
      ? `Set-Content -LiteralPath '${resumedFile.replaceAll("'", "''")}' -Value RECONNECTED\r`
      : `printf RECONNECTED > '${resumedFile.replaceAll("'", "'\\''")}'\r`;
    value(await client.page.evaluate(input => window.shellfoxRemote!.request({ peerId: 'workstation', method: 'writeTerminal', payload: input }), { ...identity, data: resumedCommand }));
    await expect.poll(async () => (await readFile(resumedFile, 'utf8').catch(() => '')).trim()).toBe('RECONNECTED');
    const remote = value(await client.page.evaluate(() => window.shellfoxRemote!.request({ peerId: 'workstation', method: 'getSnapshot', payload: {} })));
    expect(remote.sessions.some(item => item.id === session.id)).toBe(true);
    value(await host.page.evaluate(() => window.shellfoxRemote!.revoke({ peerId: 'laptop' })));
    await expect.poll(async () => (await state(client.page)).peers.find(peer => peer.id === 'workstation')?.connection).toBe('rejected');
    expect((await client.page.evaluate(() => window.shellfoxRemote!.request({ peerId: 'workstation', method: 'getSnapshot', payload: {} }))).ok).toBe(false);
    value(await host.page.evaluate(input => window.shellfox.closeTab!(input), identity));
  } finally {
    // Cleanup is scoped to the applications and terminals created by this test.
    if (host) await host.page.evaluate(async () => {
      const result = await window.shellfox.getSnapshot();
      if (result.ok) for (const session of result.value.sessions) for (const tab of session.tabs) {
        if (tab.generation && tab.lifecycle === 'open') await window.shellfox.closeTab!({ tabId: tab.id, generation: tab.generation });
      }
    }).catch(() => {});
    for (const app of applications) await app.close().catch(() => {});
  }
});
