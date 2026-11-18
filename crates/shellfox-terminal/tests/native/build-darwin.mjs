import { mkdir, readFile, writeFile, chmod, lstat } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

if (process.platform !== 'darwin') throw new Error('Build and validate the retained Darwin supervisor on macOS.');
const root = fileURLToPath(new URL('../../../../', import.meta.url));
const arch = process.env.SHELLFOX_NATIVE_ARCH ?? process.arch;
if (!['x64', 'arm64'].includes(arch) || arch !== process.arch) throw new Error('Native acceptance requires the actual matching macOS architecture.');
const out = path.resolve(process.env.SHELLFOX_NATIVE_OUTPUT ?? path.join(root, 'tmp/terminal-native', `darwin-${arch}`));
// Build outputs stay isolated from installed/production resources.
const relative = path.relative(path.join(root, 'tmp'), out);
if (!relative || relative.startsWith('..') || path.isAbsolute(relative)) throw new Error('Native build output must be a child of the project tmp directory.');
let current = path.join(root, 'tmp');
for (const segment of ['', ...relative.split(path.sep)]) {
  current = path.join(current, segment);
  try {
    const info = await lstat(current);
    if (!info.isDirectory() || info.isSymbolicLink()) throw new Error('Native output directory must not traverse a symlink.');
  } catch (error) { if (error.code !== 'ENOENT') throw error; }
}
await mkdir(out, { recursive: true });
for (const leaf of ['shellfox-terminal-supervisor', 'supervisor-manifest.json']) {
  try {
    const info = await lstat(path.join(out, leaf));
    if (!info.isFile() || info.isSymbolicLink()) throw new Error('Native output must not replace a symlink or special file.');
  } catch (error) { if (error.code !== 'ENOENT') throw error; }
}
const source = path.join(root, 'src/main/terminal/native/darwin-terminal-supervisor.c');
const binary = path.join(out, 'shellfox-terminal-supervisor');
function run(file, args) {
  const result = spawnSync(file, args, { encoding: 'utf8', timeout: 60000, maxBuffer: 4096 });
  if (result.error || result.status !== 0) throw new Error(`${file} failed: ${result.error?.message ?? result.stderr}`);
  return result.stdout;
}
run('/usr/bin/xcrun', ['--sdk', 'macosx', 'clang', '-std=c11', '-O2', '-Wall', '-Wextra', '-Werror',
  '-arch', arch === 'arm64' ? 'arm64' : 'x86_64', '-mmacosx-version-min=13.0', source, '-lproc', '-o', binary]);
await chmod(binary, 0o755);
run('/usr/bin/lipo', ['-verify_arch', arch === 'arm64' ? 'arm64' : 'x86_64', binary]);
const capability = JSON.parse(run(binary, ['--capabilities']));
const expected = { version: 1, platform: 'darwin', arch, available: true, reason: null, ownedSession: true, guardians: true, termination: true };
if (Object.keys(capability).sort().join() !== Object.keys(expected).sort().join() || Object.entries(expected).some(([key, value]) => capability[key] !== value)) throw new Error('Retained supervisor native capability validation failed.');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
await writeFile(path.join(out, 'supervisor-manifest.json'), JSON.stringify({ version: 1, platform: 'darwin', arch,
  source: path.relative(root, source), sourceSha256: hash(await readFile(source)),
  resourcePath: 'terminal-native/shellfox-terminal-supervisor', sha256: hash(await readFile(binary)) }, null, 2) + '\n');
console.log(binary);
