import { _electron, expect, type ElectronApplication, type Page } from '@playwright/test';
import { mkdir } from 'node:fs/promises';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import type { ManagerSnapshot, Result } from '../../src/shared/contracts';
export const root = path.resolve('.');
export const executable = path.join(root, 'tmp/packages/Shellfox-win32-x64/Shellfox.exe');
export const testMain = path.join(root, 'tmp/build-test/main/index.cjs');
export async function scratch(label: string) {
  const dir = path.join(root, 'tmp/verification', `${label}-${randomUUID()}`);
  await mkdir(dir, { recursive: true });
  return dir;
}
export const env = () => ({ ...process.env, SHELLFOX_TEST_MODE: '1', SHELLFOX_TEST_ROOT: root, TEMP: path.join(root, 'tmp'), TMP: path.join(root, 'tmp') });
export async function launch(userData: string, backend: 'fake' | 'real' = 'fake', packaged = false, extra: string[] = []) {
  const app = await _electron.launch({ ...(packaged ? { executablePath: executable } : {}), cwd: root, env: env(), args: [...(packaged ? [] : [testMain]), '--test-user-data', userData, '--test-backend', backend, ...extra], timeout: 30000 });
  const page = await app.firstWindow();
  await page.waitForFunction(() => !!window.shellfox);
  await expect(page.getByRole('main', { name: 'Workspace' })).toBeVisible();
  if (process.env.SHELLFOX_TEST_VISIBLE !== '1') {
    expect(await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().every(window => !window.isVisible() && !window.isFocused() && !window.isFocusable()))).toBe(true);
  }
  return { app, page };
}
export async function snapshot(page: Page): Promise<ManagerSnapshot> {
  const result = await page.evaluate(() => window.shellfox.getSnapshot());
  return value(result);
}
export function value<T>(result: Result<T>): T {
  expect(result.ok, JSON.stringify(result)).toBe(true);
  if (!result.ok) throw new Error(result.error.message);
  return result.value;
}
export async function observe(app: ElectronApplication, agents = 0, rootState: 'alive' | 'exited' | 'unavailable' = 'alive', health: 'healthy' | 'unknown' = 'healthy') {
  await app.evaluate((_electron, args) => (globalThis as any).__shellfoxTest.backend.observe(...args), [agents, rootState, health]);
}
export async function calls(app: ElectronApplication) {
  return app.evaluate(() => {
    const backend = (globalThis as any).__shellfoxTest.backend;
    return { launches: backend.launches.length, focuses: backend.focuses.length, watch: backend.watch.length };
  });
}
