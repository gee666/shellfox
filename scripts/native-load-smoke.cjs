const assert = require('node:assert/strict');
const Database = require('better-sqlite3');
const { verifyBuiltUnixPty } = require('./pty-runtime.cjs');
const runtime = process.platform === 'win32' ? null : verifyBuiltUnixPty(require);
const pty = require('node-pty');
const db = new Database(':memory:');
assert.equal(db.prepare('SELECT 42 AS value').get().value, 42);
db.close();
assert.equal(typeof pty.spawn, 'function');
if (process.platform === 'win32') {
  require('../node_modules/node-pty/build/Release/conpty.node');
  require('../node_modules/node-pty/build/Release/conpty_console_list.node');
} else {
  require('../node_modules/node-pty/build/Release/pty.node');
}
const passed = () => console.log('Electron native load passed: ' + process.versions.electron + ' ' + process.platform + '-' + process.arch + ', SQLite and node-pty.');
if (process.platform === 'darwin') {
  // Loading pty.node alone cannot establish spawn-helper readiness. Exercise the
  // same built pair in pinned Electron before any supervisor lifecycle test.
  console.log('Darwin built PTY prerequisite: ' + JSON.stringify(runtime));
  const terminal = pty.spawn('/bin/sh', ['-c', 'exit 7'], { cwd: process.cwd(), env: { ...process.env }, name: 'xterm-256color' });
  const timer = setTimeout(() => { console.error('Darwin built PTY prerequisite exit timed out.'); process.exit(1); }, 10000);
  terminal.onData(() => {});
  terminal.onExit(event => {
    clearTimeout(timer);
    assert.equal(event.exitCode, 7);
    passed();
  });
} else passed();
