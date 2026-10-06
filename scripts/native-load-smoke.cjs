const assert = require('node:assert/strict');
const Database = require('better-sqlite3');
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
console.log('Electron native load passed: ' + process.versions.electron + ' ' + process.platform + '-' + process.arch + ', SQLite and node-pty.');
