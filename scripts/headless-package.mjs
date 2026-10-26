import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { root, run } from './common.mjs';
import { buildHeadless } from './headless-build.mjs';
import { headlessInventory } from './headless-inventory.mjs';
import { smokeHeadless } from './headless-smoke.mjs';
import { fileSha256, runtimeTar } from './node-runtime.mjs';

export async function packageHeadless() {
  const version = JSON.parse(readFileSync('package.json', 'utf8')).version;
  const name = `shellfox-headless-${version}-${process.platform}-${process.arch}.${process.platform === 'win32' ? 'zip' : 'tar.gz'}`;
  const artifacts = path.join(root, 'tmp/headless-artifacts');
  await mkdir(artifacts, { recursive: true });
  const archive = path.join(artifacts, name);
  await rm(archive, { force: true });
  await rm(archive + '.sha256', { force: true });
  const directory = await buildHeadless();
  headlessInventory(directory);
  if (process.platform === 'win32') {
    const quote = value => "'" + value.replaceAll("'", "''") + "'";
    run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', `$ErrorActionPreference='Stop'; Compress-Archive -LiteralPath ${quote(directory)} -DestinationPath ${quote(archive)} -CompressionLevel Optimal`], 300000);
  } else run(runtimeTar(), ['-czf', archive, '-C', path.dirname(directory), path.basename(directory)], 300000);
  await writeFile(archive + '.sha256', `${await fileSha256(archive)}  ${name}\n`);
  // Test the archive, not the source staging tree, from a path containing spaces.
  const unpack = await mkdtemp(path.join(root, 'tmp/headless archive smoke '));
  try {
    run(runtimeTar(), ['-xf', archive, '-C', unpack], 120000);
    await smokeHeadless(path.join(unpack, path.basename(directory)));
  } catch (error) {
    await rm(archive, { force: true });
    await rm(archive + '.sha256', { force: true });
    throw error;
  } finally { await rm(unpack, { recursive: true, force: true }); }
  console.log(`[headless-package] ${archive}`);
  return archive;
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await packageHeadless();
