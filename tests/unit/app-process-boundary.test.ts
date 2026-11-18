import * as prefixed from 'node:child_process';
import * as bare from 'child_process';
import { createRequire } from 'node:module';
import { createRequire as bareCreateRequire } from 'module';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { promisify } from 'node:util';
import { expect, it, vi } from 'vitest';
import { takeBlockedProcesses, unitChildProcess, unitModule } from './app-process-model';
import { registerEndpointRoot } from './endpoint-permission-model';

it.each([['node:child_process', prefixed], ['child_process', bare]] as const)('denies all unmodeled process entry points through %s', async (_id, cp) => {
  const apis = ['spawn', 'fork', 'exec', 'execFile', 'spawnSync', 'execSync', 'execFileSync'] as const;
  // If mock resolution ever changes, fail before calling a real builtin.
  for (const name of apis) expect(cp[name]).toBe(unitChildProcess[name]);
  expect(cp.ChildProcess).toBe(unitChildProcess.ChildProcess);
  for (const name of apis) expect(() => Reflect.apply(cp[name], cp, ['synthetic-never-executed'])).toThrow('UNIT_APP_PROCESS_DENIED');
  expect(() => new unitChildProcess.ChildProcess().spawn()).toThrow('UNIT_APP_PROCESS_DENIED');
  expect(() => new cp.ChildProcess().kill()).toThrow('UNIT_APP_PROCESS_DENIED');
  await expect(promisify(cp.execFile)('synthetic-never-executed', [])).rejects.toThrow('UNIT_APP_PROCESS_DENIED');
  expect(takeBlockedProcesses()).toEqual([
    'spawn', 'fork', 'exec', 'execFile callback', 'spawnSync', 'execSync', 'execFileSync',
    'ChildProcess.spawn', 'unscripted model kill', 'unregistered execFile protocol',
  ]);
});

it('denies the reviewed CJS actor routes before a require function is returned', () => {
  expect(createRequire).toBe(unitModule.createRequire);
  expect(bareCreateRequire).toBe(unitModule.createRequire);
  expect(() => createRequire(import.meta.url)).toThrow('UNIT_APP_PROCESS_DENIED');
  expect(() => bareCreateRequire(import.meta.url)).toThrow('UNIT_APP_PROCESS_DENIED');
  expect(takeBlockedProcesses()).toEqual(['CJS actor graph HOLD', 'CJS actor graph HOLD']);
});

it('rejects a hypothetical helper identically when its image is absent or present', async () => {
  expect(prefixed.execFile).toBe(unitChildProcess.execFile);
  await mkdir('tmp', { recursive: true });
  const root = await mkdtemp(path.resolve('tmp/unit-process-denial-'));
  try {
    const file = path.join(root, 'shellfox-tailscale.exe');
    const invocation = () => promisify(prefixed.execFile)(file, ['--private-permissions', root, 'directory', 'protect']);
    await expect(invocation()).rejects.toThrow('UNIT_APP_PROCESS_DENIED');
    await writeFile(file, 'not an executable', { flag: 'wx' });
    await expect(invocation()).rejects.toThrow('UNIT_APP_PROCESS_DENIED');
    expect(takeBlockedProcesses()).toEqual(['unregistered execFile protocol', 'unregistered execFile protocol']);
  } finally { await rm(root, { recursive: true, force: true }); }
});

it('allows only the registered binary path, operation and fixture target', async () => {
  expect(prefixed.execFile).toBe(unitChildProcess.execFile);
  await mkdir('tmp', { recursive: true });
  const root = await mkdtemp(path.resolve('tmp/unit-permission-denial-'));
  try {
    const resources = path.join(root, 'synthetic-resources'), directory = path.join(root, 'host');
    const image = 'shellfox-tailscale' + (process.platform === 'win32' ? '.exe' : '');
    const expected = path.join(resources, 'tailscale', image), wrong = path.join(root, image);
    vi.stubEnv('SHELLFOX_RESOURCES_PATH', resources);
    const reply = { stdout: '{"ok":false}', stderr: '' };
    registerEndpointRoot(root, ['host'], [{ kind: 'directory', action: 'protect', reply }]);
    const model = promisify(prefixed.execFile), options = { windowsHide: true, timeout: 8000, maxBuffer: 1024 };
    await expect(model(wrong, ['--private-permissions', directory, 'directory', 'protect'], options)).rejects.toThrow('UNIT_APP_PROCESS_DENIED');
    await expect(model(expected, ['--private-permissions', root, 'directory', 'protect'], options)).rejects.toThrow('UNIT_APP_PROCESS_DENIED');
    await expect(model(expected, ['--private-permissions', directory, 'directory', 'check'], options)).rejects.toThrow('UNIT_APP_PROCESS_DENIED');
    await expect(model(expected, ['--private-permissions', directory, 'directory', 'protect'], options)).resolves.toEqual(reply);
    expect(takeBlockedProcesses()).toEqual(Array.from({ length: 3 }, () => 'unregistered execFile protocol'));
  } finally { vi.unstubAllEnvs(); await rm(root, { recursive: true, force: true }); }
});
