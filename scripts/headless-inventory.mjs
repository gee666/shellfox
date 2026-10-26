import assert from 'node:assert/strict';
import { lstatSync, readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';

export function headlessInventory(directory, platform = process.platform) {
  const files = [];
  function walk(folder) {
    for (const name of readdirSync(folder)) {
      const file = path.join(folder, name), stat = lstatSync(file);
      assert.ok(!stat.isSymbolicLink(), 'Package must not reference outside files: ' + file);
      if (stat.isDirectory()) walk(file);
      else files.push(path.relative(directory, file).replaceAll('\\', '/'));
    }
  }
  walk(directory);
  for (const file of files) {
    assert.ok(!file.startsWith('resources/elevated-terminal/'), 'Unused administrator runtime in headless package: ' + file);
    assert.ok(!/(^|\/)(electron|chromium|chrome-sandbox|chrome_100_percent.pak|react|react-dom|renderer|preload|vite|\.vite)(\/|$)/i.test(file), 'UI file in headless package: ' + file);
    assert.ok(!/\.(asar|tsx|html|css|map|pdb)$/i.test(file), 'Build or UI file in headless package: ' + file);
    if (file.endsWith('/package.json')) {
      const manifest = JSON.parse(readFileSync(path.join(directory, file), 'utf8'));
      for (const dependency of Object.keys(manifest.dependencies ?? {})) assert.ok(!/^(electron|react|react-dom|@xterm\/xterm|@xterm\/addon-fit)$/.test(dependency), 'UI dependency: ' + dependency);
    }
  }
  const required = ['app/main.cjs', 'app/package.json', 'shellfox', 'shellfox.cmd', 'shellfox.ps1', 'README.txt', 'build.json', 'runtime.json', 'service/install.mjs',
    `bin/node${platform === 'win32' ? '.exe' : ''}`, 'bin/NODE-LICENSE', 'app/node_modules/better-sqlite3/build/Release/better_sqlite3.node',
    'app/node_modules/node-pty/build/Release/pty.node', `resources/tailscale/shellfox-tailscale${platform === 'win32' ? '.exe' : ''}`,
    'resources/tailscale/THIRD-PARTY-NOTICES.txt'];
  if (platform === 'darwin') required.push('app/node_modules/node-pty/build/Release/spawn-helper', 'resources/terminal-native/shellfox-process-snapshot', 'resources/terminal-native/shellfox-terminal-supervisor');
  if (platform === 'win32') required.push('util/pagent.exe', 'app/node_modules/node-pty/build/Release/conpty.node', 'app/node_modules/node-pty/build/Release/conpty/conpty.dll', 'app/node_modules/node-pty/build/Release/conpty/OpenConsole.exe');
  for (const file of required) assert.ok(files.includes(file), 'Missing headless package file: ' + file);
  const modules = readdirSync(path.join(directory, 'app/node_modules')).sort();
  assert.deepEqual(modules, ['better-sqlite3', 'node-pty'], 'Only native runtime dependencies may be external');
  return files.sort();
}
