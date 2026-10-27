// Test-owned PTY only. The Playwright worker never loads a native terminal addon.
const { createRequire } = require('node:module');
const { realpathSync } = require('node:fs');
const path = require('node:path');
const { createInterface } = require('node:readline');

const root = path.resolve(__dirname, '../..');
const entry = path.join(root, 'tmp/build-test/cli/main/cli.cjs');
let terminal, exited = false, failed = false, stopping = false;
let lifetime, shutdown;
function emit(event, done) { process.stdout.write(JSON.stringify(event) + '\n', done); }
function exitDriver(code) {
  clearTimeout(lifetime); clearTimeout(shutdown);
  process.stdout.write('', () => process.stderr.write('', () => process.exit(code)));
}
function fail(message) {
  failed = true; emit({ type: 'error', message });
  if (!terminal || exited) { exitDriver(1); return; }
  stop();
}
function stop() {
  if (stopping || exited) return;
  stopping = true;
  try { terminal.kill(); }
  catch (error) { failed = true; emit({ type: 'error', message: 'Owned PTY kill failed: ' + String(error) }); }
  shutdown = setTimeout(() => {
    // This is a driver failure, never an invented terminal exit code.
    emit({ type: 'error', message: 'Owned PTY did not report exit after kill.' }, () => exitDriver(1));
  }, 5000);
}
function record(value) { return value !== null && typeof value === 'object' && !Array.isArray(value); }
function exactKeys(value, keys) { return record(value) && Object.keys(value).sort().join(',') === [...keys].sort().join(','); }
function within(parent, target) {
  const relative = path.relative(parent, target);
  return relative !== '' && relative !== '..' && !relative.startsWith('..' + path.sep) && !path.isAbsolute(relative);
}
function ownedDirectory(directory) {
  if (typeof directory !== 'string' || !path.isAbsolute(directory) || /[\0\r\n]/.test(directory)) throw new Error('Invalid test directory.');
  const allowed = realpathSync(path.join(root, 'tmp/verification'));
  if (!within(allowed, realpathSync(directory))) throw new Error('PTY directories must be inside project tmp/verification.');
  return directory;
}
function configuration(raw) {
  if (!exactKeys(raw, ['userData', 'cwd', 'args']) || !Array.isArray(raw.args) || raw.args.length < 2 || raw.args.length > 3 ||
    !['terminals', 'tm'].includes(raw.args[0]) || raw.args[1] !== 'attach' ||
    (raw.args.length === 3 && (typeof raw.args[2] !== 'string' || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(raw.args[2])))) throw new Error('Only isolated CLI attach invocations are allowed.');
  return { userData: ownedDirectory(raw.userData), cwd: ownedDirectory(raw.cwd), args: raw.args };
}
try {
  if (process.argv.length !== 3 || process.argv[2].length > 65536) throw new Error('Expected one encoded test configuration.');
  const config = configuration(JSON.parse(Buffer.from(process.argv[2], 'base64').toString('utf8')));
  const load = createRequire(entry), expected = realpathSync(path.join(root, 'tmp/build-test/cli/node_modules/node-pty'));
  const resolved = realpathSync(path.dirname(load.resolve('node-pty/package.json')));
  if (resolved !== expected) throw new Error('node-pty must come from the built CLI bundle.');
  const pty = load('node-pty');
  terminal = pty.spawn(process.execPath, [entry, '--user-data', config.userData, ...config.args], {
    cwd: config.cwd, env: process.env, name: 'xterm-256color', cols: 90, rows: 40,
    // Use the same staged ConPTY DLL as the real backend, not legacy console-list enumeration.
    useConpty: true, useConptyDll: true,
  });
  terminal.onData(data => emit({ type: 'data', data }));
  terminal.onExit(event => {
    exited = true; clearTimeout(lifetime); clearTimeout(shutdown);
    emit({ type: 'exit', exitCode: event.exitCode, ...(event.signal === undefined ? {} : { signal: event.signal }) }, () => exitDriver(failed ? 1 : 0));
  });
  emit({ type: 'ready', pid: terminal.pid, runtime: process.execPath, module: resolved });
  lifetime = setTimeout(() => fail('Owned PTY exceeded its 120 second lifetime.'), 120000);
  const input = createInterface({ input: process.stdin, crlfDelay: Infinity });
  input.on('line', line => {
    try {
      if (Buffer.byteLength(line) > 512 * 1024) throw new Error('Driver command is too large.');
      const command = JSON.parse(line);
      if (!record(command) || !Number.isSafeInteger(command.id) || command.id < 1 || exited) throw new Error('Invalid driver command.');
      if (command.method === 'write' && exactKeys(command, ['id', 'method', 'data']) && typeof command.data === 'string' && Buffer.byteLength(command.data) <= 65536) terminal.write(command.data);
      else if (command.method === 'kill' && exactKeys(command, ['id', 'method'])) stop();
      else throw new Error('Only write and kill are permitted.');
      emit({ type: 'ack', id: command.id, method: command.method });
    } catch (error) { fail(error instanceof Error ? error.message : 'Invalid driver command.'); }
  });
  input.on('close', () => { if (!exited) fail('Driver input closed before the owned PTY exited.'); });
  process.once('SIGTERM', () => { if (!exited) { failed = true; emit({ type: 'error', message: 'Driver terminated before owned PTY exit.' }); stop(); } });
} catch (error) { fail(error instanceof Error ? error.message : 'PTY driver could not start.'); }
process.on('uncaughtException', error => fail('Driver uncaught exception: ' + error.message));
process.on('unhandledRejection', error => fail('Driver unhandled rejection: ' + String(error)));
