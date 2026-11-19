import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, readdirSync, lstatSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const root = fileURLToPath(new URL('../', import.meta.url));
export const scratchRoot = path.join(root, 'tmp');
export const targets = Object.freeze({
  'win32-x64': 'x86_64-pc-windows-msvc',
  'win32-arm64': 'aarch64-pc-windows-msvc',
  'linux-x64': 'x86_64-unknown-linux-gnu',
  'linux-arm64': 'aarch64-unknown-linux-gnu',
  'darwin-x64': 'x86_64-apple-darwin',
  'darwin-arm64': 'aarch64-apple-darwin',
});

export function compilerHost() {
  const host = run('rustc', ['-vV']).match(/^host: (.+)$/m)?.[1];
  if (!host) throw new Error('rustc did not report its host triple');
  return host;
}

export function nativeTarget() {
  const target = targets[`${process.platform}-${process.arch}`];
  if (!target) throw new Error('Rust portable packages require native Windows/Linux/macOS x64 or arm64.');
  const actual = compilerHost();
  if (actual !== target) throw new Error(`Native compiler required: expected ${target}, found ${actual}`);
  return target;
}

export function run(command, args, options = {}) {
  mkdirSync(scratchRoot, { recursive: true });
  const result = spawnSync(command, args, {
    cwd: root, encoding: 'utf8', timeout: 30 * 60_000, windowsHide: true,
    maxBuffer: 128 * 1024 * 1024,
    ...options,
    env: { ...process.env, TMP: scratchRoot, TEMP: scratchRoot, TMPDIR: scratchRoot, ...options.env },
  });
  if (result.error || result.status !== 0) {
    throw new Error(`${command} ${args.join(' ')} failed: ${result.error?.message ?? result.signal ?? result.status}\n${result.stderr ?? ''}\n${result.stdout ?? ''}`);
  }
  return result.stdout;
}

export const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
export const fileHash = file => sha256(readFileSync(file));
export const readJson = file => JSON.parse(readFileSync(file, 'utf8'));
export const executableName = name => name + (process.platform === 'win32' ? '.exe' : '');

export function filesUnder(directory, prefix = '') {
  return readdirSync(directory).sort().flatMap(name => {
    const relative = prefix ? `${prefix}/${name}` : name;
    const full = path.join(directory, name);
    const stat = lstatSync(full);
    if (stat.isSymbolicLink()) throw new Error(`Symlinks are not allowed in portable packages: ${full}`);
    if (stat.isDirectory()) return filesUnder(full, relative);
    if (!stat.isFile()) throw new Error(`Non-regular package file: ${full}`);
    return [relative];
  });
}

export function assertHeadlessNames(names) {
  const forbidden = /^(?:gpui(?:[-_].*)?|wgpu(?:[-_].*)?|ash|blade[-_].*|metal|vulkano(?:[-_].*)?|vulkan[-_].*|gpu[-_].*|glow|d3d12|winit|glutin(?:[-_].*)?|electron(?:[-_].*)?|naga|skia[-_].*|shellfox-desktop)$/i;
  const leaks = names.filter(name => forbidden.test(name));
  if (leaks.length) throw new Error(`Headless dependency leak: ${[...new Set(leaks)].join(', ')}`);
}

export function assertHeadlessFiles(files) {
  const forbidden = /(?:^|\/)(?:node_modules|node(?:\.exe|\.dll)?|electron(?:\.exe)?|app\.asar|chrome-sandbox|icudtl\.dat|libEGL\.[^/]+|libGLESv2\.[^/]+|vk_swiftshader[^/]*|vulkan[^/]*|resources\/elevated-terminal)(?:\/|$)|\.(?:node|pak)$/i;
  const leaks = files.filter(file => forbidden.test(file));
  if (leaks.length) throw new Error(`Headless artifact leak: ${leaks.join(', ')}`);
}

export function selectedBinaries(metadata, kind) {
  const spec = kind === 'desktop'
    ? [['shellfox-desktop', 'shellfox'], ['shellfox-cli', 'shellfox-cli']]
    : [['shellfox-cli', 'shellfox-cli']];
  return spec.map(([packageName, bin]) => {
    const pkg = metadata.packages.find(entry => entry.name === packageName && metadata.workspace_members.includes(entry.id));
    if (!pkg?.targets.some(target => target.name === bin && target.kind.includes('bin'))) {
      throw new Error(`Required workspace binary not available: ${packageName}/${bin}. Coordinate runtime names before packaging.`);
    }
    return { packageName, bin, version: pkg.version };
  });
}
