const assert = require('node:assert/strict');
const path = require('node:path');
const { createRequire } = require('node:module');
const { randomUUID } = require('node:crypto');
const { stripVTControlCharacters } = require('node:util');
const [asar, data] = process.argv.slice(2);
const load = createRequire(path.join(asar, 'package.json'));
const Database = load('better-sqlite3');
const db = new Database(path.join(data, 'smoke.sqlite3'));
db.exec('CREATE TABLE smoke (value TEXT NOT NULL)');
db.prepare('INSERT INTO smoke VALUES (?)').run('packaged SQLite');
assert.equal(db.prepare('SELECT value FROM smoke').get().value, 'packaged SQLite');
db.close();
const pty = load('node-pty');
const marker = 'PACKAGED_PTY_' + randomUUID().replaceAll('-', '');
const win = process.platform === 'win32';
const shell = win ? path.join(process.env.SystemRoot, 'System32/WindowsPowerShell/v1.0/powershell.exe') : '/bin/sh';
const terminal = pty.spawn(shell, win ? ['-NoLogo', '-NoProfile'] : [], {
  name: 'xterm-256color', cols: 80, rows: 24, cwd: data,
  env: { ...process.env, TERM: 'xterm-256color' }, useConpty: true, useConptyDll: true,
});
let output = '', printed = false, exited = false;
const timer = setTimeout(() => {
  console.error('Packaged PTY output/exit timed out.');
  // Do not use node-pty's Unix numeric-PID kill fallback. This no-profile
  // fixture starts no jobs; request voluntary exit through its owned input.
  try { if (win) terminal.kill(); else terminal.write('exit\r'); } catch {}
  process.exitCode = 1;
}, 15000);
terminal.onData(chunk => {
  output = (output + chunk).slice(-65536);
  const plain = stripVTControlCharacters(output);
  const size = win ? /(?:^|[\r\n])SIZE:100:40(?:[\r\n]|$)/ : /(?:^|[\r\n])40 100(?:[\r\n]|$)/;
  if (!printed && new RegExp('(?:^|[\\r\\n])' + marker + '(?:[\\r\\n]|$)').test(plain) && size.test(plain)) {
    printed = true;
    terminal.write('exit 7\r');
  }
});
terminal.onExit(event => {
  exited = true;
  clearTimeout(timer);
  try {
    assert.ok(printed, 'No real shell output marker.');
    assert.equal(event.exitCode, 7);
    console.log('Packaged Electron ' + process.versions.electron + ': SQLite write/read and native PTY spawn/output/resize/exit passed.');
  } catch (error) { console.error(error); process.exitCode = 1; }
  // ConPTY may retain pipe handles after its exit event; this fixture owns no
  // remaining shells, and does not need to keep the Electron Node runner alive.
  setImmediate(() => process.exit(process.exitCode || 0));
});
// Let interactive shell startup finish before sending input. No user profile is
// loaded by this dependency smoke; the backend profile smoke tests real profiles.
setTimeout(() => {
  if (exited) return;
  terminal.resize(100, 40);
  terminal.write(win ? `Write-Output '${marker}'; Write-Output ('SIZE:{0}:{1}' -f $Host.UI.RawUI.WindowSize.Width,$Host.UI.RawUI.WindowSize.Height)\r` : `printf '%s\\n' '${marker}'; stty size\r`);
}, 500);
