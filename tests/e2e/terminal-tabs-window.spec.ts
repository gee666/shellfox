import { test, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { launch, scratch, snapshot, value } from '../fixtures/electron';

test('a windowed terminal strip keeps native drag, keyboard focus and rename with bounded DOM', async () => {
  const dir = await scratch('terminal-tabs-window'), { app, page } = await launch(path.join(dir, 'data'), 'real');
  let owned: { id: string; generation: string } | undefined;
  try {
    const created = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID(), title: 'Many tabs' }));
    owned = { id: created.tabs[0].id, generation: created.tabs[0].generation! };
    // Seed metadata-only failed tabs, not hundreds of shell processes. One real
    // test-owned PTY proves tab operations leave process identity unchanged.
    const ids = await app.evaluate((_electron, input) => {
      const { repository, service } = (globalThis as any).__shellfoxTest;
      const first = repository.tabs(input.sessionId)[0];
      repository.transaction(() => input.ids.forEach((id, index) => repository.saveTab({ ...first, id, operationId: input.generations[index], ordinal: index + 1,
        title: `Window tab ${index + 2}`, lifecycle: 'launch-uncertain', error: { code: 'LAUNCH_FAILED', message: 'Metadata-only test tab.', retryable: false },
        terminal: { ...first.terminal, userTitle: `Window tab ${index + 2}` } })));
      service.changed('sessions');
      return [first.id, ...input.ids];
    }, { sessionId: created.id, ids: Array.from({ length: 199 }, () => randomUUID()), generations: Array.from({ length: 199 }, () => randomUUID()) });
    await page.getByRole('button', { name: 'Select session Many tabs', exact: true }).click();
    const strip = page.getByRole('tablist', { name: 'Terminals' });
    await expect(strip).toHaveClass(/terminal-tabs-windowed/);
    const identities = await app.evaluate(() => (globalThis as any).__shellfoxTest.backend.live().map((entry: any) => [entry.tabId, entry.generation, entry.root.pid]));
    expect(await strip.getByRole('tab').count()).toBeLessThan(25);
    const first = page.locator(`#terminal-tab-${ids[0]}`), last = page.locator(`#terminal-tab-${ids[199]}`);
    await first.focus(); await page.keyboard.press('End');
    await expect(last).toHaveAttribute('aria-selected', 'true'); await expect(last).toBeFocused();
    await expect.poll(() => last.evaluate(node => {
      const item = node.parentElement!.getBoundingClientRect(), root = node.closest('[role="tablist"]')!.getBoundingClientRect();
      return item.left >= root.left - 1 && item.right <= root.right + 1;
    })).toBe(true);
    await page.keyboard.press('F2');
    const name = page.getByRole('textbox', { name: 'Terminal tab name' });
    await name.fill('Build many'); await name.press('Enter');
    await expect(last).toContainText('Build many'); await expect(last).toBeFocused();
    await page.keyboard.press('Alt+ArrowLeft');
    await expect.poll(async () => (await snapshot(page)).sessions.find(item => item.id === created.id)!.tabs.at(-2)!.id).toBe(ids[199]);

    await strip.evaluate(node => { node.scrollLeft = 0; }); await expect(first).toBeVisible();
    const source = await first.boundingBox(); expect(source).not.toBeNull();
    await page.mouse.move(source!.x + 40, source!.y + 12); await page.mouse.down();
    await page.mouse.move(source!.x + 65, source!.y + 12, { steps: 5 });
    // Keep the native drag source mounted while replacing the visible window.
    await strip.evaluate(node => { node.scrollLeft = 150 * 180; });
    const target = page.locator(`#terminal-tab-${ids[150]}`);
    await expect(target).toBeVisible();
    const destination = await target.boundingBox(); expect(destination).not.toBeNull();
    await page.mouse.move(destination!.x + 60, destination!.y + 12, { steps: 5 });
    await page.mouse.move(destination!.x + 60, destination!.y + 12); await page.mouse.up();
    await expect.poll(async () => (await snapshot(page)).sessions.find(item => item.id === created.id)!.tabs[150].id).toBe(ids[0]);
    await expect(last).toHaveAttribute('aria-selected', 'true');
    expect(await strip.getByRole('tab').count()).toBeLessThan(25);
    expect(await app.evaluate(() => (globalThis as any).__shellfoxTest.backend.live().map((entry: any) => [entry.tabId, entry.generation, entry.root.pid]))).toEqual(identities);
  } finally {
    await page.mouse.up().catch(() => {});
    if (owned) await page.evaluate(input => window.shellfox.closeTab!(input), { tabId: owned.id, generation: owned.generation }).catch(() => {});
    await app.close();
  }
});
