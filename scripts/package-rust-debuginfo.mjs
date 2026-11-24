// Line tables for crash report backtraces (Cargo.toml [profile.release]).
// Linux keeps them in the binary, zlib-compressed. Windows PDBs and macOS
// dSYMs stay beside the build for the release workflow's symbols artifact.
import { statSync } from 'node:fs';
import path from 'node:path';
import { run } from './rust-package-common.mjs';

export const LINUX_DEBUG_LIMIT = 15 * 2 ** 20;

export function debugInfoEnv(platform, env = process.env) {
  if (platform === 'linux') {
    const flag = '-C link-arg=-Wl,--compress-debug-sections=zlib';
    return { RUSTFLAGS: [env.RUSTFLAGS, flag].filter(Boolean).join(' ') };
  }
  if (platform === 'darwin') return { CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO: 'packed' };
  return {};
}

export function assertDebugGrowth(withDebug, withoutDebug, name) {
  const growth = withDebug - withoutDebug;
  if (growth > LINUX_DEBUG_LIMIT) {
    throw new Error(`${name} line tables add ${(growth / 2 ** 20).toFixed(1)} MB, over the ${LINUX_DEBUG_LIMIT / 2 ** 20} MB limit`);
  }
  return growth;
}

/** Compares the Linux binary with a copy stripped of its debug sections. */
export function checkLinuxDebugSize(binary, work) {
  const stripped = path.join(work, `${path.basename(binary)}.nodebug`);
  run('objcopy', ['--strip-debug', binary, stripped]);
  const growth = assertDebugGrowth(statSync(binary).size, statSync(stripped).size, path.basename(binary));
  console.log(`[package-rust] ${path.basename(binary)} line tables: ${(growth / 2 ** 20).toFixed(1)} MB`);
}
