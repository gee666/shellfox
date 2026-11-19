import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { nativeTarget, root, run, scratchRoot, sha256 } from './rust-package-common.mjs';
import { wslFilesTarget } from './rust-package-wsl-files.mjs';

// Explicit CI/local acquisition only. This script never runs during app startup.
export async function withPinnedZig(useZig) {
  const manifest = JSON.parse(readFileSync(path.join(root, 'crates/ghostty-vt-sys/build-support/zig-0.17.0.json'), 'utf8'));
  const host = `${process.arch === 'x64' ? 'x86_64' : 'aarch64'}-${{ win32: 'windows', linux: 'linux', darwin: 'macos' }[process.platform]}`;
  const pin = manifest.hosts[host];
  if (!pin || !/^[a-f0-9]{64}$/.test(pin.sha256)) throw new Error(`No pinned Zig archive for ${host}`);
  mkdirSync(scratchRoot, { recursive: true });
  const work = mkdtempSync(path.join(scratchRoot, 'rust-package-prereqs-'));
  try {
    const response = await fetch(pin.url, { signal: AbortSignal.timeout(120_000) });
    if (!response.ok) throw new Error(`Zig download failed: HTTP ${response.status}`);
    const bytes = Buffer.from(await response.arrayBuffer());
    if (sha256(bytes) !== pin.sha256) throw new Error('Zig archive SHA-256 mismatch');
    const archive = path.join(work, path.basename(new URL(pin.url).pathname));
    writeFileSync(archive, bytes);
    const tar = process.platform === 'win32' ? path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/tar.exe') : 'tar';
    run(tar, ['-xf', archive, '-C', work]);
    const folder = path.basename(archive).replace(/\.(?:tar\.xz|zip)$/, '');
    return await useZig(path.join(work, folder, process.platform === 'win32' ? 'zig.exe' : 'zig'));
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
  if (process.platform === 'win32') run('rustup', ['target', 'add', wslFilesTarget()]);
  const prefix = await prepareGhostty();
  if (prefix && process.env.GITHUB_ENV) {
    // GitHub's env file is supplied by the runner, not part of the package.
    const { appendFileSync } = await import('node:fs');
    appendFileSync(process.env.GITHUB_ENV, `GHOSTTY_VT_DIR=${prefix}\n`);
  }
}
