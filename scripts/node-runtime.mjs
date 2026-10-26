// Pins are copied from https://nodejs.org/dist/v24.15.0/SHASUMS256.txt.
// Never fetch a checksum at build time. Verify archives before extracting them.
import { createHash, randomUUID } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { chmod, copyFile, mkdir, rename, rm } from 'node:fs/promises';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
const execute = promisify(execFile);
export const runtimeTar = () => process.platform === 'win32' ? path.win32.join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'tar.exe') : 'tar';
export const nodeVersion = '24.15.0';
export const nodeHashes = Object.freeze({
  'node-v24.15.0-win-x64.zip': 'cc5149eabd53779ce1e7bdc5401643622d0c7e6800ade18928a767e940bb0e62',
  'node-v24.15.0-win-arm64.zip': 'c9eb7402eda26e2ba7e44b6727fc85a8de56c5095b1f71ebd3062892211aa116',
  'node-v24.15.0-linux-x64.tar.gz': '44836872d9aec49f1e6b52a9a922872db9a2b02d235a616a5681b6a85fec8d89',
  'node-v24.15.0-linux-arm64.tar.gz': '73afc234d558c24919875f51c2d1ea002a2ada4ea6f83601a383869fefa64eed',
  'node-v24.15.0-darwin-x64.tar.gz': 'ffd5ee293467927f3ee731a553eb88fd1f48cf74eebc2d74a6babe4af228673b',
  'node-v24.15.0-darwin-arm64.tar.gz': '372331b969779ab5d15b949884fc6eaf88d5afe87bde8ba881d6400b9100ffc4',
  'node-v24.15.0-headers.tar.gz': '9a9250039fef572d0bcc110ac5e78055faf5604421211d2d5f578ebc5a34e703',
  'win-x64/node.exe': '3331e1ffe19874215472217c5e94f5a0c6d8e18c4ac7111d3937aa0ad5e9b4a5',
  'win-arm64/node.exe': '49a54c103f4919ce64199a043ef5cd309507de491d718085edee089cd8e87543',
  'win-x64/node.lib': '2dad98cd40967a4fd4a97195979e0e237e09ec292483700a51d2c20fe5a97c87',
  'win-arm64/node.lib': '5eefacb0514b137a1dfcf7b2d4d83418aca7eea4895d086e6ca9254d29f83ba8',
  LICENSE: '4573185d56580da2b890ba34a85a409257640f1c5632eade4300137266194d18',
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
