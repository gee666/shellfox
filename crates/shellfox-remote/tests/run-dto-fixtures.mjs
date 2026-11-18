// Generates real Rust adapter output, then parses it with the shared Zod schemas.
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';

const root = fileURLToPath(new URL('../../../', import.meta.url));
const tmp = join(root, 'tmp');
mkdirSync(tmp, { recursive: true });
const scratch = mkdtempSync(join(tmp, 'remote-dto-'));
const env = { ...process.env, SHELLFOX_ADAPTER_FIXTURES: join(scratch, 'fixtures.json') };
function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: root, env, stdio: 'inherit', timeout: 180_000,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} exited ${result.status}`);
}
try {
  run('cargo', ['test', '--locked', '-p', 'shellfox-remote', '--test', 'dto_adapter', 'emit_zod_fixtures', '--', '--exact']);
  run(process.execPath, [join(root, 'node_modules', 'vitest', 'vitest.mjs'),
    'run', '--config', 'crates/shellfox-remote/tests/dto-vitest.config.ts']);
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
