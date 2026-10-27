import { test, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import { access, mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { z } from 'zod';
import type { ManagerSnapshot, TabDto } from '../../src/shared/contracts';
import { connectHostClient } from '../../src/host/client';
import { launch, root, scratch, seedTerminal, snapshot, value } from '../fixtures/electron';
import { cliTtyEntry as helper, cliTtyRuntime as runtime, externalTty } from '../fixtures/cli-tty';
import { stageCliResources } from '../fixtures/cli-resources';

async function prepareBuiltCli() {
  await access(helper); await access(runtime);
  await stageCliResources(root);
}
const identities = (state: ManagerSnapshot) => state.sessions.map(session => ({ id: session.id, cwd: session.cwd,
  tabs: session.tabs.map(tab => ({ id: tab.id, generation: tab.generation, cwd: tab.cwd })).sort((a, b) => a.id.localeCompare(b.id)),
})).sort((a, b) => a.id.localeCompare(b.id));
const powershell = (tab: TabDto) => ['pwsh', 'windows-powershell'].includes(tab.profileId ?? '');
const stopClient = (client: ReturnType<typeof externalTty>) => client.close();

test('built CLI attaches only existing terminal IDs, takes control, exchanges shell input/output and detaches without creation', async ({}, testInfo) => {
  test.setTimeout(180000);
  await prepareBuiltCli();
  const dir = await scratch('cli-attach-tty'), data = path.join(dir, 'data');
  const callerCwd = path.join(dir, 'caller directory'), otherCwd = path.join(dir, 'other directory');
  await mkdir(callerCwd); await mkdir(otherCwd);
  const { app, page } = await launch(data, 'real');
  const clients: Array<ReturnType<typeof externalTty>> = [], evidence: unknown[] = [];
  let primaryFailure = false;
  async function attach(tab: TabDto, namespace = 'terminals', mustTake = false) {
    const before = identities(await snapshot(page)), args = [namespace, 'attach', tab.id];
    const tty = externalTty(data, args, callerCwd); clients.push(tty);
    await expect.poll(() => tty.raw().includes('\x1b[?1049h'), { timeout: 20000 }).toBe(true);
    const ownerGrid = '\x1b[8;40;90t';
    await expect.poll(() => tty.raw().includes(ownerGrid) || tty.text().includes('Another client is controlling this terminal.'), { timeout: 20000 }).toBe(true);
    const tookControl = !tty.raw().includes(ownerGrid);
    if (mustTake) expect(tookControl).toBe(true);
    if (tookControl) { tty.terminal.write('\x14'); await expect.poll(() => tty.raw().includes(ownerGrid), { timeout: 20000 }).toBe(true); }
    const tag = 'CLI_TTY_' + randomUUID().replaceAll('-', '') + ':';
    const command = powershell(tab)
      ? `[Console]::WriteLine(('CLI_TTY_' + '${tag.slice(8)}' + (Get-Location).Path))`
      : `printf '%s%s%s\\n' 'CLI_TTY_' '${tag.slice(8)}' "$PWD"`;
    tty.terminal.write(command + '\r');
    await expect.poll(() => tty.text().replaceAll('\n', '').includes(tag + tab.cwd), { timeout: 20000 }).toBe(true);
    expect(tty.text()).not.toContain('ParserError');
    tty.terminal.write('\x1d'); await expect.poll(tty.code, { timeout: 15000 }).toBe(0);
    const after = await snapshot(page);
    expect(identities(after)).toEqual(before);
    expect(after.sessions.flatMap(session => session.tabs).find(candidate => candidate.id === tab.id)).toMatchObject({ lifecycle: 'open', generation: tab.generation });
    expect(await app.evaluate(({ app }) => app.getPath('userData'))).toBe(data);
    evidence.push({ args, callerCwd, expectedCwd: tab.cwd, sessionId: tab.sessionId, tabId: tab.id, generation: tab.generation, output: tty.text(), exitCode: tty.code(), tookControl, noCliCreation: true, stillOpenAfterDetach: true });
  }
  try {
    // All sessions and tabs exist before any CLI process starts.
    const first = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: callerCwd, requestId: randomUUID(), title: 'CLI existing one' }));
    const second = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: otherCwd, requestId: randomUUID(), title: 'CLI existing two' }));
    const expanded = value(await page.evaluate(input => window.shellfox.addTab(input), { sessionId: first.id, cwd: otherCwd }));
    const added = expanded.tabs.find(tab => !first.tabs.some(original => original.id === tab.id));
    if (!added) throw new Error('Setup terminal missing');
    const baseline = identities(await snapshot(page));
    const invalid = externalTty(data, ['terminals', 'attach'], callerCwd); clients.push(invalid);
    await expect.poll(invalid.code, { timeout: 15000 }).toBe(2); expect(identities(await snapshot(page))).toEqual(baseline);
    evidence.push({ args: ['terminals', 'attach'], exitCode: invalid.code(), output: invalid.text(), noCliCreation: true });

    await page.getByRole('button', { name: 'Select session CLI existing one', exact: true }).click();
    await page.getByRole('tab').first().click();
    await expect(page.getByLabel('Interactive terminal')).toBeVisible();
    const tab = first.tabs[0]; if (!tab.generation) throw new Error('Missing generation');
    const marker = 'CLI_OWNER_' + randomUUID().replaceAll('-', '');
    const observed = page.evaluate(({ tabId, marker }) => new Promise<boolean>(resolve => {
      let text = '';
      const finish = (seen: boolean) => { clearTimeout(timer); unsubscribe(); resolve(seen); };
      const unsubscribe = window.shellfox.subscribeTerminal!(event => {
        if (event.type === 'data' && event.tabId === tabId) { text = (text + event.data).slice(-16384); if (text.includes(marker)) finish(true); }
      });
      const timer = setTimeout(() => finish(false), 15000);
      document.documentElement.dataset.cliOwnerProbe = 'ready';
    }), { tabId: tab.id, marker });
    await page.waitForFunction(() => document.documentElement.dataset.cliOwnerProbe === 'ready');
    const ownerProbe = powershell(tab) ? `[Console]::WriteLine(('CLI_OWNER_' + '${marker.slice(10)}'))\r` : `printf '%s%s\\n' 'CLI_OWNER_' '${marker.slice(10)}'\r`;
    value(await seedTerminal(app, { tabId: tab.id, generation: tab.generation, data: ownerProbe }));
    expect(await observed).toBe(true);
    await attach(tab, 'terminals', true);
    await attach(second.tabs[0], 'tm');
    await attach(added);
    await attach(tab, 'tm');
    expect(identities(await snapshot(page))).toEqual(baseline);
  } catch (error) { primaryFailure = true; throw error; }
  finally {
    const closed = await Promise.allSettled(clients.map(stopClient));
    try {
      await testInfo.attach('external-tty-evidence.json', { body: JSON.stringify({ userData: data, helper, runtime, platform: process.platform, steps: evidence, clients: clients.map(client => client.evidence()), cleanupErrors: closed.flatMap(result => result.status === 'rejected' ? [String(result.reason)] : []) }, null, 2), contentType: 'application/json' });
    } finally { await app.close(); }
    if (!primaryFailure) for (const result of closed) if (result.status === 'rejected') throw result.reason;
  }
});

test('existing-terminal CLI takeover forces a same-grid TUI repaint without manually resizing the external TTY', async ({}, testInfo) => {
  test.setTimeout(90000); await prepareBuiltCli();
  const dir = await scratch('cli-same-grid'), data = path.join(dir, 'data');
  const script = path.join(dir, 'same-grid.cjs'), record = path.join(dir, 'resize.json'), marker = 'SAME_GRID_' + randomUUID().replaceAll('-', '');
  await writeFile(script, `const fs = require('node:fs');
let resizeCount = 0;
function paint() {
  const state = { resizeCount, cols: process.stdout.columns, rows: process.stdout.rows };
  fs.writeFileSync(process.argv[2] + '.new', JSON.stringify(state));
  fs.renameSync(process.argv[2] + '.new', process.argv[2]);
  process.stdout.write('\\x1b[2J\\x1b[H' + process.argv[3] + ':' + state.cols + 'x' + state.rows + ':r' + state.resizeCount);
}
process.stdout.on('resize', () => { resizeCount++; paint(); });
process.stdin.setRawMode(true); process.stdin.resume();
process.stdin.on('data', bytes => { if (bytes.includes(3)) process.exit(0); });
paint(); setInterval(() => {}, 1000);
`);
  const { app, page } = await launch(data, 'real'), standIn = await connectHostClient(data);
  let tty: ReturnType<typeof externalTty> | undefined, primaryFailure = false;
  const resizeSchema = z.object({ resizeCount: z.number().int().nonnegative(), cols: z.number().int().positive(), rows: z.number().int().positive() }).strict();
  const readResize = async () => resizeSchema.parse(JSON.parse(await readFile(record, 'utf8')));
  try {
    const session = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID(), title: 'Same grid TUI' }));
    const tab = session.tabs[0]; if (!tab.generation) throw new Error('Missing generation');
    const identity = { tabId: tab.id, generation: tab.generation };
    let attachment = value(await standIn.service.attachTerminal(identity));
    if (attachment.control === 'blocked') { value(await standIn.service.takeTerminalControl(identity)); attachment = value(await standIn.service.attachTerminal(identity)); }
    if (attachment.control !== 'owned') throw new Error('No setup owner');
    value(await standIn.service.resizeTerminal({ ...identity, controlEpoch: attachment.controlEpoch, cols: 90, rows: 40 }));
    const quote = (text: string) => "'" + text.replaceAll("'", powershell(tab) ? "''" : "'\\''") + "'";
    const command = (powershell(tab) ? '& ' : '') + [runtime, script, record, marker].map(quote).join(' ') + '\r';
    value(await seedTerminal(app, { ...identity, data: command }));
    await expect.poll(async () => (await readResize().catch(() => null))?.cols, { timeout: 15000 }).toBe(90);
    const before = await readResize();
    tty = externalTty(data, ['terminals', 'attach', tab.id], dir);
    const external = tty;
    await expect.poll(() => external.text().includes('Another client is controlling this terminal.'), { timeout: 15000 }).toBe(true);
    external.terminal.write('\x14');
    // No external.terminal.resize call: the attach implementation must provoke repaint itself.
    await expect.poll(async () => (await readResize()).resizeCount, { timeout: 20000 }).toBeGreaterThan(before.resizeCount);
    await expect.poll(async () => ({ cols: (await readResize()).cols, rows: (await readResize()).rows }), { timeout: 15000 }).toEqual({ cols: 90, rows: 40 });
    await expect.poll(() => external.text().replaceAll('\n', '').includes(marker + ':90x40'), { timeout: 15000 }).toBe(true);
    expect(external.text()).not.toContain('ParserError');
    const settled = await readResize();
    await expect.poll(() => external.text().replaceAll('\n', '').includes(marker + ':90x40:r' + settled.resizeCount), { timeout: 15000 }).toBe(true);
    const outputAtRest = external.raw().length;
    // Pass the former lease interval with both viewers attached. There must be
    // no automatic size renewal, ownership switch or terminal-reply feedback.
    await new Promise<void>(resolve => setTimeout(resolve, 31000));
    expect(await readResize()).toEqual(settled);
    expect(external.raw().slice(outputAtRest)).toBe('');
    await testInfo.attach('same-grid-repaint.json', { body: JSON.stringify({ before, after: await readResize(), tabId: tab.id, generation: tab.generation, output: external.text(), manuallyResizedExternalTty: false, stableWithoutRenewalMs: 31000 }, null, 2), contentType: 'application/json' });
    external.terminal.write('\x1d'); await expect.poll(external.code, { timeout: 15000 }).toBe(0);
  } catch (error) { primaryFailure = true; throw error; }
  finally {
    const closed = await Promise.allSettled(tty ? [stopClient(tty)] : []);
    try {
      await testInfo.attach('same-grid-driver.json', { body: JSON.stringify({ driver: tty?.evidence(), cleanupErrors: closed.flatMap(result => result.status === 'rejected' ? [String(result.reason)] : []) }, null, 2), contentType: 'application/json' });
    } finally { try { await standIn.close(); } finally { await app.close(); } }
    if (!primaryFailure) for (const result of closed) if (result.status === 'rejected') throw result.reason;
  }
});
