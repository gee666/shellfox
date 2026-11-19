import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { cpSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { privateBuildPlan } from './gpui-build.mjs';
import { developmentHelperPath, ensureDevelopmentHelper } from './tailscale.mjs';

const root = fileURLToPath(new URL('../', import.meta.url));
const args = process.argv.slice(2);
const mode = args.shift() ?? 'shell';
if (!['shell', 'fixture', 'smoke'].includes(mode) || args.some(arg => arg !== '--release')) {
  console.error('Usage: node scripts/gpui-experiment.mjs [shell|fixture|smoke] [--release]');
  process.exitCode = 2;
} else {
  try {
    const data = process.env.SHELLFOX_EXPERIMENT_DATA_DIR ?? path.join(root, 'tmp', 'gpui-experiment');
    if (!path.isAbsolute(data)) throw new Error('SHELLFOX_EXPERIMENT_DATA_DIR must be absolute and experiment-only.');
    const runtimeArgs = [`--${mode}`, '--data-dir', data];
    if (mode === 'shell') {
      const supplied = process.env.SHELLFOX_EXPERIMENT_HELPER;
      if (process.platform === 'win32' && supplied &&
          path.resolve(supplied).toLowerCase() !== developmentHelperPath().toLowerCase()) {
        throw new Error(`Windows dev launches must use ${developmentHelperPath()}.`);
      }
      const development = supplied ? null : ensureDevelopmentHelper();
      const helper = development?.executable ?? supplied;
      const digest = (development?.sha256 ?? process.env.SHELLFOX_EXPERIMENT_HELPER_SHA256)?.toLowerCase();
      if (!path.isAbsolute(helper) || !existsSync(helper)) throw new Error('The overridden helper must be an existing absolute path.');
      if (!digest || !/^[a-f0-9]{64}$/.test(digest)) throw new Error('An overridden helper requires a trusted SHELLFOX_EXPERIMENT_HELPER_SHA256.');
      runtimeArgs.push('--helper', helper, '--helper-sha256', digest);
    }
    // Never replace an executable that may now be a user-occupied window.
    const { directory, executable, args: cargo } = privateBuildPlan(root, { release: args.includes('--release') });
    mkdirSync(directory, { recursive: true });
    const build = spawn('cargo', cargo, { cwd: root, stdio: 'inherit', shell: false });
    build.on('error', error => { console.error(`Could not build workspace: ${error.message}`); process.exitCode = 1; });
    build.on('exit', (code, signal) => {
      if (code !== 0 || signal) { process.exitCode = code ?? 1; return; }
      try {
        const output = path.resolve(root, process.env.CARGO_TARGET_DIR ?? 'target', args.includes('--release') ? 'release' : 'debug');
        if (process.platform === 'win32') {
          // Cargo verified this pinned build input. Copy bytes, not a symlink into a live app.
          const bytes = readFileSync(path.join(output, 'ghostty-vt.dll'));
          writeFileSync(path.join(directory, 'ghostty-vt.dll'), bytes, { flag: 'wx' });
          const source = createHash('sha256').update(bytes).digest('hex');
          const copied = createHash('sha256').update(readFileSync(path.join(directory, 'ghostty-vt.dll'))).digest('hex');
          if (source !== copied) throw new Error('Copied Ghostty DLL digest mismatch.');
        }
        const notices = path.join(output, 'ghostty-vt-notices');
        if (existsSync(notices)) cpSync(notices, path.join(directory, 'ghostty-vt-notices'), { recursive: true, errorOnExist: true });
        console.error(`Private experiment executable: ${executable}`);
        const app = spawn(executable, runtimeArgs, { cwd: root, stdio: 'inherit', shell: false });
        app.on('error', error => { console.error(`Could not run workspace: ${error.message}`); process.exitCode = 1; });
        app.on('exit', (status, stopped) => { if (stopped) console.error(`Workspace exited on ${stopped}`); process.exitCode = status ?? 1; });
      } catch (error) { console.error(error instanceof Error ? error.message : String(error)); process.exitCode = 1; }
    });
  } catch (error) { console.error(error instanceof Error ? error.message : String(error)); process.exitCode = 1; }
}
