import path from 'node:path';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { afterEach, expect, it, vi } from 'vitest';
import { runHostService } from './service';
const roots: string[] = [];
afterEach(async () => { vi.unstubAllEnvs(); for (const root of roots.splice(0)) await rm(root, { force: true, recursive: true }); });
it.each(['desktop', 'headless'])('delegates %s service installation to its packaged installer without touching OS startup', async mode => {
  await mkdir('tmp', { recursive: true }); const root = await mkdtemp(path.resolve('tmp/cli-service-')); roots.push(root);
  const directory = mode === 'desktop' ? path.join(root, 'resources', 'cli', 'service') : path.join(root, 'service');
  await mkdir(directory, { recursive: true });
  await writeFile(path.join(directory, 'install.mjs'), 'console.log(JSON.stringify({args:process.argv.slice(2),runAsNode:process.env.ELECTRON_RUN_AS_NODE??null,runtime:process.env.SHELLFOX_RUNTIME_PATH??null}));');
  vi.stubEnv('SHELLFOX_RUNTIME_PATH', mode === 'headless' ? root : undefined);
  vi.stubEnv('SHELLFOX_RESOURCES_PATH', path.join(root, 'resources'));
  vi.stubEnv('ELECTRON_RUN_AS_NODE', '1');
  const result = await runHostService(['install', '--startup'], path.join(root, 'data path'), { json: true, yes: false });
  expect(result).toMatchObject({ operation: 'install' });
  if (!result || typeof result !== 'object' || !('message' in result) || typeof result.message !== 'string') throw new Error('Missing installer result');
  expect(JSON.parse(result.message)).toEqual({ args: ['install', '--user-data', path.join(root, 'data path'), '--startup'], runAsNode: process.versions.electron ? '1' : null, runtime: mode === 'headless' ? root : null });
});
it('never hides confirmation in JSON or silently manages a remote service', async () => {
  await expect(runHostService(['uninstall'], process.cwd(), { json: true, yes: false })).rejects.toThrow('--yes');
  await expect(runHostService(['install'], process.cwd(), { json: true, yes: true, host: 'peer' })).rejects.toThrow('local host');
});
