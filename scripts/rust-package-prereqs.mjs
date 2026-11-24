import { appendFileSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { pinnedZigArchive, zigPin } from '../crates/ghostty-vt-sys/build-support/zig.mjs';
import { nativeTarget, run, scratchRoot } from './rust-package-common.mjs';
import { wslFilesTarget } from './rust-package-wsl-files.mjs';

// Explicit CI/local acquisition only. This script never runs during app startup.
// Extracts the verified pinned Zig into an empty directory and returns the executable path.
export async function extractPinnedZig(directory) {
  const pin = zigPin();
  const data = await pinnedZigArchive(pin);
  mkdirSync(directory, { recursive: true });
  const archive = path.join(directory, pin.name);
  writeFileSync(archive, data, { flag: 'wx' });
  const tar = process.platform === 'win32' ? path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/tar.exe') : 'tar';
  run(tar, ['-xf', archive, '-C', directory]);
  rmSync(archive);
  const folder = pin.name.replace(/\.(?:tar\.xz|zip)$/, '');
  return path.join(directory, folder, process.platform === 'win32' ? 'zig.exe' : 'zig');
}

export async function withPinnedZig(useZig) {
  mkdirSync(scratchRoot, { recursive: true });
  const work = mkdtempSync(path.join(scratchRoot, 'rust-package-prereqs-'));
  try {
    return await useZig(await extractPinnedZig(work));
  } finally { rmSync(work, { recursive: true, force: true }); }
}

export async function prepareGhostty() {
  const target = nativeTarget();
  run(process.execPath, ['crates/ghostty-vt-sys/build-support/prepare.mjs', 'fetch']);
  if (target === 'x86_64-pc-windows-msvc') {
    run(process.execPath, ['crates/ghostty-vt-sys/build-support/prepare.mjs', 'verify']);
    return null; // The build script validates/extracts the official pinned x64 DLL.
  }
  const prefix = path.join(scratchRoot, 'rust-package-ghostty', target);
  if (existsSync(prefix)) throw new Error(`Use a new Ghostty prefix; refusing stale installation: ${prefix}`);
  return withPinnedZig(zig => {
    run(process.execPath, ['crates/ghostty-vt-sys/build-support/prepare.mjs', 'build', '--target', target, '--prefix', prefix, '--zig', zig]);
    console.log(`Set GHOSTTY_VT_DIR=${prefix}`);
    return prefix;
  });
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { values } = parseArgs({ options: { 'zig-only': { type: 'string' } } });
  if (values['zig-only']) {
    // Prints only the executable path, for builds that need Zig but not the native Ghostty.
    const directory = path.resolve(values['zig-only']);
    if (existsSync(directory)) throw new Error(`Zig directory must not exist: ${directory}`);
    console.log(await extractPinnedZig(directory));
  } else {
    if (process.platform === 'win32') run('rustup', ['target', 'add', wslFilesTarget()]);
    const prefix = await prepareGhostty();
    // GitHub's env file is supplied by the runner, not part of the package.
    if (prefix && process.env.GITHUB_ENV) appendFileSync(process.env.GITHUB_ENV, `GHOSTTY_VT_DIR=${prefix}\n`);
  }
}
