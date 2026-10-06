import { chmodSync, cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { runNode, root } from './common.mjs';
import { darwinHelpers } from './terminal-native-helpers.mjs';
import { sourceDigest } from './source-digest.mjs';
const mode = process.argv[2];
// A running Windows package locks its images. Allow a fresh, project-tmp output
// without deleting or stopping the user's current installation.
const alternate = process.env.SHELLFOX_PACKAGE_OUT_DIR ? path.resolve(process.env.SHELLFOX_PACKAGE_OUT_DIR) : null;
if (alternate) {
  const relative = path.relative(path.join(root, 'tmp'), alternate);
  if (!relative || relative.startsWith('..') || path.isAbsolute(relative)) throw new Error('Alternate package output must be inside project tmp.');
}
const platform = process.platform, arch = process.arch;
if (!['win32', 'linux', 'darwin'].includes(platform) || !['x64', 'arm64'].includes(arch)) throw new Error('Package on the target OS with target-native x64/arm64 Node. Cross-staging native binaries is not supported.');
if (mode !== 'make' && mode !== 'package') throw new Error('Unknown Forge command');
runNode('scripts/build.mjs', [], 600000);
const sourceStamp = JSON.parse(readFileSync(path.join(root, 'tmp/build-source-stamp.json'), 'utf8'));
if (sourceDigest(root) !== sourceStamp.sourceHash) throw new Error('Sources changed after build; rebuild before packaging.');
// Staging avoids recursively copying the repository's tmp tree into Packager.
const stage = path.join(root, 'tmp/package-source');
rmSync(stage, { recursive: true, force: true });
mkdirSync(path.join(stage, 'tmp'), { recursive: true });
cpSync('tmp/build', path.join(stage, 'tmp/build'), { recursive: true });
if (platform === 'darwin') {
  const resources = path.join(root, 'tmp/package-resources/terminal-native');
  rmSync(resources, { recursive: true, force: true });
  mkdirSync(resources, { recursive: true });
  for (const { basename } of darwinHelpers) {
    cpSync(path.join(root, 'tmp/terminal-native', `darwin-${arch}`, basename), path.join(resources, basename));
    chmodSync(path.join(resources, basename), 0o755);
  }
}
const sourceManifest = JSON.parse(readFileSync('package.json', 'utf8'));
const manifest = Object.fromEntries(['name','productName','version','description','author','license','main'].map(key => [key,sourceManifest[key]]));
// Renderer packages and Zod are bundled; these modules load native code lazily.
manifest.dependencies = Object.fromEntries(['better-sqlite3', 'node-pty'].map(name => [name, sourceManifest.dependencies[name]]));
manifest.devDependencies = { electron: sourceManifest.devDependencies.electron };
manifest.config = { forge: './forge.config.cjs' };
writeFileSync(path.join(stage, 'package.json'), JSON.stringify(manifest, null, 2));
writeFileSync(path.join(stage, 'forge.config.cjs'), 'const base = require(' + JSON.stringify(path.join(root, 'forge.config.cjs')) + '); module.exports = ' + (alternate ? '{ ...base, outDir: ' + JSON.stringify(alternate) + ' }' : 'base') + ';\n');
const sqliteTarget = `${platform}-${arch}.node`;
for (const name of ['better-sqlite3', 'node-addon-api', 'node-pty']) {
  const base = path.join(root, 'node_modules', name);
  cpSync(base, path.join(stage, 'node_modules', name), {
    recursive: true,
    filter: source => {
      const relative = path.relative(base, source).replace(/\\/g, '/');
      if (name === 'node-addon-api') return true;
      if (!relative || relative === 'package.json' || /^LICENSE/.test(relative)) return true;
      if (relative === 'lib' || relative.startsWith('lib/')) return !/\.test\.|\.map$/.test(relative);
      if (name === 'better-sqlite3') {
        return relative === 'prebuilds' || relative === `prebuilds/${sqliteTarget}` ||
          ['build', 'build/Release', 'build/Release/better_sqlite3.node'].includes(relative);
      }
      // node-pty helpers need their original relative paths, not just *.node.
      if (relative === 'build' || relative === 'build/Release') return true;
      if (!relative.startsWith('build/Release/')) return false;
      return relative === 'build/Release/conpty' || /\.(node|dll|exe)$/.test(relative) || relative === 'build/Release/spawn-helper';
    },
  });
}
const ptyRelease = path.join(stage, 'node_modules/node-pty/build/Release');
for (const file of platform === 'win32'
  ? ['conpty.node', 'conpty_console_list.node', 'pty.node', 'winpty.dll', 'winpty-agent.exe', 'conpty/conpty.dll', 'conpty/OpenConsole.exe']
  : platform === 'darwin' ? ['pty.node', 'spawn-helper'] : ['pty.node']) {
  if (!existsSync(path.join(ptyRelease, file))) throw new Error('Missing target-native PTY runtime: ' + file);
}
const sqlite = path.join(stage, 'node_modules/better-sqlite3');
if (!existsSync(path.join(sqlite, 'prebuilds', sqliteTarget)) && !existsSync(path.join(sqlite, 'build/Release/better_sqlite3.node'))) throw new Error('Missing target-native SQLite runtime.');
if (platform !== 'win32') {
  // Native builds on Unix should already have these modes; normalize the staged
  // tree too so archive extraction cannot carry world-writable helper files.
  function modes(dir) {
    chmodSync(dir, 0o755);
    for (const name of readdirSync(dir)) {
      const file = path.join(dir, name);
      if (statSync(file).isDirectory()) modes(file);
      else chmodSync(file, name === 'spawn-helper' ? 0o755 : 0o644);
    }
  }
  modes(path.join(stage, 'node_modules'));
}
runNode('node_modules/@electron-forge/cli/dist/electron-forge.js', [mode, stage, `--platform=${platform}`, `--arch=${arch}`], 600000);
if (sourceDigest(root) !== sourceStamp.sourceHash) throw new Error('Sources changed during packaging; generated artifacts are stale, rebuild them.');
writeFileSync(path.join(root, 'tmp/package-source-stamp.json'), JSON.stringify(sourceStamp, null, 2));
if (alternate) writeFileSync(path.join(alternate, 'shellfox-source-stamp.json'), JSON.stringify(sourceStamp, null, 2));
