import { expect, it } from 'vitest';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import { z } from 'zod';
const driver = path.resolve('tests/fixtures/cli-tty-driver.cjs');
const errorEvent = z.object({ type: z.literal('error'), message: z.string().min(1) }).strict();
it.each([
  { userData: process.cwd(), cwd: process.cwd(), args: ['host', 'stop'] },
  { userData: process.cwd(), cwd: process.cwd(), args: ['terminals', 'attach', 'not-a-uuid'] },
  { userData: process.cwd(), cwd: process.cwd(), args: ['terminals', 'attach'] },
  { userData: process.cwd(), cwd: process.cwd(), args: ['terminals', 'attach'], executable: 'never-run' },
])('rejects unsafe driver configuration before opening a PTY: %j', config => {
  const result = spawnSync(process.execPath, [driver, Buffer.from(JSON.stringify(config)).toString('base64')], { encoding: 'utf8', timeout: 5000, windowsHide: true });
  expect(result.error).toBeUndefined(); expect(result.status).toBe(1); expect(result.stderr).toBe('');
  const events = result.stdout.trim().split('\n').map(line => errorEvent.parse(JSON.parse(line)));
  expect(events).toHaveLength(1); expect(result.stdout).not.toContain('"type":"exit"');
});
it('does not let a test-owned cwd authorize an unrelated user-data directory', async () => {
  await mkdir('tmp/verification', { recursive: true });
  const cwd = await mkdtemp(path.resolve('tmp/verification/driver-validation-'));
  try {
    const config = { userData: process.cwd(), cwd, args: ['tm', 'attach'] };
    const result = spawnSync(process.execPath, [driver, Buffer.from(JSON.stringify(config)).toString('base64')], { encoding: 'utf8', timeout: 5000, windowsHide: true });
    expect(result.error).toBeUndefined(); expect(result.status).toBe(1);
    expect(errorEvent.parse(JSON.parse(result.stdout)).message).toContain('inside project tmp/verification');
  } finally { await rm(cwd, { recursive: true, force: true }); }
});
