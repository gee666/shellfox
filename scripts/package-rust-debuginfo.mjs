// Line tables for crash report backtraces (Cargo.toml [profile.release]) stay
// out of the packages: Windows PDBs, macOS dSYMs and Linux .debug files go to
// the release workflow's symbols artifact. Embedded, Linux line tables would
// add about 57 MB (compressed) to each binary.
import path from 'node:path';
import { run } from './rust-package-common.mjs';

export function debugInfoEnv(platform) {
  return platform === 'darwin' ? { CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO: 'packed' } : {};
}

/** Moves a Linux binary's debug sections to `<binary>.debug`, linked by CRC. */
export function splitLinuxDebugInfo(binary) {
  const debug = `${binary}.debug`;
  run('objcopy', ['--only-keep-debug', '--compress-debug-sections=zlib', binary, debug]);
  run('objcopy', ['--strip-debug', `--add-gnu-debuglink=${path.basename(debug)}`, binary], { cwd: path.dirname(binary) });
}
