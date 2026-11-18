import { mkdir, mkdtemp, rm, writeFile, chmod } from 'node:fs/promises';
import path from 'node:path';
import { describe, expect, it, vi } from 'vitest';
import { probeTerminalScope, scopedCommand } from './linux-scope';
import { PtyBackend } from './backend';
import { factoryFixture, launchInput } from './test-fixtures';

describe('Linux terminal scopes', () => {
  it('wraps the command after systemd-run options so its own arguments stay intact', () => {
    const wrapped = scopedCommand('/usr/bin/systemd-run', { file: '/bin/bash', args: ['-l', '-i', '--user'], cwd: '/work' });
    expect(wrapped.file).toBe('/usr/bin/systemd-run'); expect(wrapped.cwd).toBe('/work');
    expect(wrapped.args).toContain('--property=OOMPolicy=continue');
    expect(wrapped.args.slice(wrapped.args.indexOf('--'))).toEqual(['--', '/bin/bash', '-l', '-i', '--user']);
    expect(wrapped.args.find(arg => arg.startsWith('--unit='))).toMatch(/^--unit=shellfox-terminal-[0-9a-f-]{36}$/);
  });
  it.skipIf(process.platform !== 'linux')('launches directly when systemd-run is missing or cannot reach a user manager', async () => {
    await mkdir('tmp', { recursive: true });
    const dir = await mkdtemp(path.resolve('tmp/shellfox-scope-'));
    try {
      const exec = vi.fn(async () => { throw new Error('Failed to connect to bus'); });
      expect(await probeTerminalScope(dir, async () => {})).not.toBe(path.join(dir, 'systemd-run'));
      await writeFile(path.join(dir, 'systemd-run'), '#!/bin/sh\n'); await chmod(path.join(dir, 'systemd-run'), 0o755);
      expect(await probeTerminalScope(dir, exec)).toBeNull();
      const [file, args] = exec.mock.calls[0] as unknown as [string, string[]];
      expect(file).toBe(path.join(dir, 'systemd-run'));
      expect(args.slice(0, -1).filter(arg => !arg.startsWith('--unit='))).toEqual(scopedCommand(file, { file: 'true', args: [] }).args.slice(0, -1).filter(arg => !arg.startsWith('--unit=')));
      expect(await probeTerminalScope(`relative:${dir}`, async () => {})).toBe(path.join(dir, 'systemd-run'));
    } finally { await rm(dir, { recursive: true, force: true }); }
  });
  it('passes the systemd-run wrapper to the PTY only for local Linux shells', async () => {
    const f = factoryFixture();
    const backend = new PtyBackend({ ...f.options, platform: 'linux', terminalScope: async () => '/usr/bin/systemd-run' });
    expect((await backend.initialize()).ok).toBe(true);
    expect((await backend.launch(launchInput())).ok).toBe(true);
    const [file, args] = vi.mocked(f.factory).mock.calls[0]!;
    expect(file).toBe('/usr/bin/systemd-run'); expect(args.slice(args.indexOf('--'))).toEqual(['--', '/bin/bash', '-l', '-i']);
    const windows = factoryFixture(), direct = new PtyBackend({ ...windows.options, terminalScope: async () => '/usr/bin/systemd-run' });
    expect((await direct.initialize()).ok).toBe(true); expect((await direct.launch(launchInput())).ok).toBe(true);
    expect(vi.mocked(windows.factory).mock.calls[0]![0]).toBe('/bin/bash');
    await backend.dispose(); await direct.dispose();
  });
});
