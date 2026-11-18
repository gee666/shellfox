import path from 'node:path';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { afterEach, expect, it } from 'vitest';
import { z } from 'zod';
import { readAuthKey } from './secrets';
import { publishPrivateEndpoint } from '../main/control-endpoint';
const roots: string[] = [];
afterEach(async () => { for (const root of roots.splice(0)) await rm(root, { force: true, recursive: true }); });
async function fixture() { await mkdir('tmp', { recursive: true }); const root = await mkdtemp(path.resolve('tmp/cli-input-')); roots.push(root); return root; }
// OWNED INTEGRATION HOLD: this file is never selected by default unit.
it('reads a protected raw auth key without requiring a JSON credentials file', async () => {
  const root = await fixture(), file = path.join(root, 'private', 'auth-key');
  await publishPrivateEndpoint(root, file, z.object({ secret: z.string() }), { secret: 'placeholder' });
  await writeFile(file, ['tskey', 'auth', 'synthetic-secret'].join('-') + '\n');
  expect(await readAuthKey(file)).toBe(['tskey', 'auth', 'synthetic-secret'].join('-'));
});
