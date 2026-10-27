// Runtime pins are copied from https://nodejs.org/dist/v24.22.1/SHASUMS256.txt.
// LICENSE is hashed from the same release tag.
// Never fetch a checksum at build time. Verify archives before extracting them.
import { createHash, randomUUID } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { chmod, copyFile, mkdir, rename, rm } from 'node:fs/promises';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
const execute = promisify(execFile);
export const runtimeTar = () => process.platform === 'win32' ? path.win32.join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'tar.exe') : 'tar';
export const nodeVersion = '24.22.1';
export const nodeHashes = Object.freeze({
  'node-v24.22.1-win-x64.zip': 'd2046d28c2c3dcd211ac5145c0bf30e2047ef9dfb306693c8c6e2eccd8b95fa5',
  'node-v24.22.1-win-arm64.zip': 'a0c807319fede24c4a0d8f84564c82a619bd313b16b10c007c383c2609e99171',
  'node-v24.22.1-linux-x64.tar.gz': 'eb6728e77c576a35162c122c45ccadf224d89ff00abc8dae46cc2897c6002d7c',
  'node-v24.22.1-linux-arm64.tar.gz': '33c37be234ba8bee041961047b19160f4d7d0f164e8e246f78bdd10feff148b3',
  'node-v24.22.1-darwin-x64.tar.gz': 'd132557e7ee2b85af08b0c8c34338e65c6430f2238d01f727728ef42dc83b035',
  'node-v24.22.1-darwin-arm64.tar.gz': '89c682593d76cd44233fa53b1563264e7de9cbb189cad169135c5b4fadfbc0a7',
  'node-v24.22.1-headers.tar.gz': '322e0a27db9088093ab8347bdc8c7e0aaa21dc9ed548390bf06b394494c09267',
  'win-x64/node.exe': 'd702daa815eabbff763c7d3808e235cf031cf2ca70211b56847bc72f51e240e5',
  'win-arm64/node.exe': '8bea91dc437fe01e7d92ef0efc6c9eaf654369aaba9b2d3e9841552ea7b95a00',
  'win-x64/node.lib': 'a0a84aa03917b578d286010b7521837e9ff1136ffb2e4a406c14316c33fd06f7',
  'win-arm64/node.lib': '2c0c3215d59c09d7c136da4949696dae121a299e9bdfc64cd9130a58610af63d',
  LICENSE: '5888dbb9a1d2b18f2c3e6c5f6af1b39de658372b402a0577b002777f14c62ace',
});
export async function fileSha256(file) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest('hex');
}
export async function matches(file, expected) {
  try { return await fileSha256(file) === expected; }
  catch (error) { if (error.code === 'ENOENT') return false; throw error; }
}
export async function downloadRuntimeAsset(url, file) {
  const curl = process.platform === 'win32' ? path.win32.join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'curl.exe') : 'curl';
  await execute(curl, ['--fail', '--location', '--proto', '=https', '--tlsv1.2', '--connect-timeout', '30', '--max-time', '120', '--retry', '2', '--retry-delay', '1', '--silent', '--show-error', '--output', file, url], { timeout: 370000, windowsHide: true, maxBuffer: 65536 });
}
export async function verifiedAsset(file, { download = downloadRuntimeAsset, cache = path.resolve('tmp/node-runtime-cache'), log = console.log } = {}) {
  const expected = nodeHashes[file];
  if (!expected) throw new Error('Unsupported Node runtime asset: ' + file);
  await mkdir(cache, { recursive: true });
  const local = path.join(cache, file.replaceAll('/', '-'));
  if (await matches(local, expected)) return local;
  const partial = local + '.' + randomUUID() + '.part';
  const url = file === 'LICENSE' ? `https://raw.githubusercontent.com/nodejs/node/v${nodeVersion}/LICENSE` : `https://nodejs.org/dist/v${nodeVersion}/${file}`;
  try {
    log(`[node-runtime] downloading ${url}`);
    await download(url, partial);
    const actual = await fileSha256(partial);
    if (actual !== expected) throw new Error(`Node runtime checksum mismatch for ${file}: expected ${expected}, received ${actual}`);
    await rename(partial, local);
    return local;
  } finally { await rm(partial, { force: true }); }
}
export function runtimeArchive(platform = process.platform, arch = process.arch) {
  if (!['win32', 'darwin', 'linux'].includes(platform) || !['x64', 'arm64'].includes(arch)) throw new Error('Node runtime requires Windows, macOS or Linux x64/arm64.');
  return `node-v${nodeVersion}-${platform === 'win32' ? 'win' : platform}-${arch}.${platform === 'win32' ? 'zip' : 'tar.gz'}`;
}
async function extractVerified(file, directory, options) {
  const archive = await verifiedAsset(file, options);
  await rm(directory, { recursive: true, force: true });
  await mkdir(directory, { recursive: true });
  // bsdtar ships with supported Windows runners and handles zip as well as tar.
  await execute(runtimeTar(), ['-xf', archive, '-C', directory], { timeout: 120000, windowsHide: true });
}
export async function stageNodeRuntime(target, { platform = process.platform, arch = process.arch, ...options } = {}) {
  const archive = runtimeArchive(platform, arch);
  const extract = path.resolve('tmp/node-runtime-extract', `${platform}-${arch}`);
  await extractVerified(archive, extract, options);
  const folder = path.join(extract, archive.replace(/\.(zip|tar\.gz)$/, ''));
  await mkdir(target, { recursive: true });
  const executable = path.join(target, platform === 'win32' ? 'node.exe' : 'node');
  await copyFile(path.join(folder, platform === 'win32' ? 'node.exe' : 'bin/node'), executable);
  await copyFile(path.join(folder, 'LICENSE'), path.join(target, 'NODE-LICENSE'));
  await chmod(executable, 0o755);
  await rm(extract, { recursive: true, force: true });
  return executable;
}
export async function stageNodeHeaders(options = {}) {
  const target = path.resolve('tmp/node-runtime-headers');
  await extractVerified(`node-v${nodeVersion}-headers.tar.gz`, target, options);
  const headers = path.join(target, `node-v${nodeVersion}`);
  if (process.platform === 'win32') {
    const library = await verifiedAsset(`win-${process.arch}/node.lib`, options);
    for (const folder of [process.arch, 'Release']) {
      await mkdir(path.join(headers, folder), { recursive: true });
      await copyFile(library, path.join(headers, folder, 'node.lib'));
    }
  }
  return headers;
}
