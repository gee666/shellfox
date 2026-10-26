import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { env, root } from './common.mjs';
import { readRuntime } from './headless-service.mjs';
import { smokeCliHost } from './cli-host-smoke.mjs';

export async function smokeDesktopCli(resources, desktopExecutable) {
  const cli = path.resolve(resources, 'cli');
  const runtime = readRuntime(cli);
  assert.equal(runtime.headless, false);
  assert.equal(runtime.resources, path.resolve(resources));
  if (process.platform !== 'win32') assert.equal(runtime.node, path.resolve(desktopExecutable));
  const environment = { ...env };
  for (const key of ['DISPLAY', 'WAYLAND_DISPLAY', 'SHELLFOX_HEADLESS', 'SHELLFOX_RESOURCES_PATH', 'SHELLFOX_RUNTIME_PATH', 'SHELLFOX_PROJECT_ROOT', 'ELECTRON_RUN_AS_NODE', 'NODE_PATH', 'NODE_OPTIONS']) delete environment[key];
  if (runtime.runAsNode) environment.ELECTRON_RUN_AS_NODE = '1';
  const cwd = await mkdtemp(path.join(root, 'tmp/desktop CLI unrelated cwd '));
  try {
    const result = spawnSync(runtime.node, [path.join(root, 'scripts/node-runtime-native-smoke.cjs'), runtime.entry, path.join(cli, 'node_modules'), runtime.runAsNode ? 'electron' : 'node'], { cwd, env: environment, timeout: 20000, encoding: 'utf8', windowsHide: true });
    assert.equal(result.error, undefined, result.error?.message);
    assert.equal(result.status, 0, result.stderr || result.stdout);
    // Deliberately no resource/project environment overrides. The installed CLI
    // must find its own manifest and helpers regardless of the caller's cwd.
    await smokeCliHost({ executable: runtime.node, entry: runtime.entry, cwd, environment });
  } finally { await rm(cwd, { recursive: true, force: true }); }
  console.log('[desktop-cli-smoke] native dependencies and foreground host passed');
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (!process.argv[2]) throw new Error('Usage: node scripts/desktop-cli-host-smoke.mjs RESOURCES [DESKTOP_EXECUTABLE]');
  await smokeDesktopCli(path.resolve(process.argv[2]), process.argv[3]);
}
