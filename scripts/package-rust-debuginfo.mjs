// Line tables for crash report backtraces (Cargo.toml [profile.release]).
// Linux keeps them in the binary, zlib-compressed after the link, which works
// whatever linker cargo picked. Windows PDBs and macOS dSYMs stay beside the
// build for the release workflow's symbols artifact.
import { statSync } from 'node:fs';
import path from 'node:path';
import { run } from './rust-package-common.mjs';

export const LINUX_DEBUG_LIMIT = 15 * 2 ** 20;

export function debugInfoEnv(platform) {
  return platform === 'darwin' ? { CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO: 'packed' } : {};
}

export function assertDebugGrowth(withDebug, withoutDebug, name, sections = () => '') {
  const growth = withDebug - withoutDebug;
  if (growth > LINUX_DEBUG_LIMIT) {
    throw new Error(`${name} line tables add ${(growth / 2 ** 20).toFixed(1)} MB, over the ${LINUX_DEBUG_LIMIT / 2 ** 20} MB limit\n${sections()}`);
  }
  return growth;
}

/**
 * Compresses a Linux binary's debug sections in place and drops variable
 * location lists (from the C and Zig objects), which backtraces never read.
 * Then checks what is left against a copy stripped of all debug sections.
 */
export function compactLinuxDebugInfo(binary, work) {
  run('objcopy', ['--compress-debug-sections=zlib', '--remove-section=.debug_loc', '--remove-section=.debug_loclists', binary]);
  const stripped = path.join(work, `${path.basename(binary)}.nodebug`);
  run('objcopy', ['--strip-debug', binary, stripped]);
  const sections = () => run('readelf', ['-SW', binary]).split('\n').filter(line => line.includes('.debug')).join('\n');
  const name = path.basename(binary);
  const growth = assertDebugGrowth(statSync(binary).size, statSync(stripped).size, name, sections);
  console.log(`[package-rust] ${name} line tables: ${(growth / 2 ** 20).toFixed(1)} MB`);
}
