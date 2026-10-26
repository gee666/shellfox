const assert = require('node:assert/strict');
const { realpathSync } = require('node:fs');
const { createRequire } = require('node:module');
const path = require('node:path');
const [entry, modules, runtime = 'node'] = process.argv.slice(2);
assert.ok(entry && modules, 'Expected entry and isolated node_modules paths');
assert.equal(Boolean(process.versions.electron), runtime === 'electron');
const load = createRequire(path.resolve(entry));
for (const name of ['node-pty', 'better-sqlite3']) {
  assert.equal(realpathSync(path.dirname(load.resolve(name + '/package.json'))), realpathSync(path.join(modules, name)), 'Native dependency resolved outside staged CLI: ' + name);
}
const db = new (load('better-sqlite3'))(':memory:');
assert.equal(db.prepare('select 42 as n').get().n, 42);
db.close();
const pty = load('node-pty');
const terminal = pty.spawn(process.platform === 'win32' ? process.env.ComSpec || 'cmd.exe' : '/bin/sh', process.platform === 'win32' ? ['/d', '/c', 'echo shellfox-native-smoke'] : ['-c', 'printf shellfox-native-smoke'], { cols: 80, rows: 24, cwd: process.cwd(), env: process.env });
let output = '';
const timeout = setTimeout(() => { terminal.kill(); process.exit(1); }, 10000);
terminal.onData(data => { output += data; });
terminal.onExit(({ exitCode }) => {
  clearTimeout(timeout);
  assert.equal(exitCode, 0);
  assert.match(output, /shellfox-native-smoke/);
  console.log(`CLI SQLite and PTY passed in ${runtime}, ${process.platform}-${process.arch}`);
  // node-pty's Windows console-list worker can retain a MessagePort after exit.
  process.exit(0);
});
