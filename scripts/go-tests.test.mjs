import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import path from 'node:path';
import { test } from 'node:test';
import { runGoTests } from './go-tests.mjs';

function fixture(t, output) {
  mkdirSync('tmp', { recursive: true });
  const root = mkdtempSync(path.resolve('tmp/go-test-runner-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const calls = [];
  const execute = (file, args, options) => {
    calls.push({ file, args, options });
    return { status: 0, stdout: args[0] === 'list' ? output(root) : '' };
  };
  const options = { command: 'go', cwd: root, env: { FIXTURE: 'value' }, root, platform: 'win32', arch: 'x64' };
  return { root, calls, execute, options };
}

test('Windows tests compile and run from reusable paths with each package working directory', t => {
  const f = fixture(t, root => `shellfox.local/helper\t${root}\nshellfox.local/helper/nethealth\t${path.join(root, 'nethealth')}\nshellfox.local/helper/nethealth\t${path.join(root, 'nethealth')}\n\n`);
  for (let attempt = 0; attempt < 2; attempt++) runGoTests(f.options, f.execute);
  assert.equal(f.calls.length, 10);
  const first = f.calls.slice(0, 5), second = f.calls.slice(5);
  assert.deepEqual(first, second);
  for (const [compile, run] of [[first[1], first[2]], [first[3], first[4]]]) {
    assert.equal(compile.file, 'go');
    assert.ok(compile.args.includes('-c'));
    assert.ok(compile.args.includes('-trimpath'));
    const executable = compile.args[compile.args.indexOf('-o') + 1];
    assert.ok(executable.startsWith(path.join(f.root, 'tmp', 'go-tests', 'win32-x64') + path.sep));
    assert.equal(run.file, executable);
    assert.deepEqual(run.args, ['-test.count=1', '-test.timeout=60s']);
    assert.equal(run.options.windowsHide, true);
    assert.deepEqual(run.options.env, f.options.env);
  }
  assert.equal(first[2].options.cwd, f.root);
  assert.equal(first[4].options.cwd, path.join(f.root, 'nethealth'));
  assert.notEqual(first[2].file, first[4].file);
});

test('Windows compile failure never executes a stale binary or falls back to go test', t => {
  const f = fixture(t, root => `shellfox.local/helper/nethealth\t${root}\n`);
  assert.throws(() => runGoTests(f.options, (file, args, options) => {
    const result = f.execute(file, args, options);
    return args[0] === 'test' ? { status: 1, stderr: 'busy executable' } : result;
  }), /busy executable/);
  assert.equal(f.calls.length, 2);
  assert.ok(f.calls.every(call => call.file === 'go'));
});

test('Windows skips packages without tests and rejects invalid package paths', t => {
  const empty = fixture(t, () => '\n');
  runGoTests(empty.options, empty.execute);
  assert.equal(empty.calls.length, 1);
  const invalid = fixture(t, root => `../escape\t${root}\n`);
  assert.throws(() => runGoTests(invalid.options, invalid.execute), /Invalid Go test package path/);
  assert.equal(invalid.calls.length, 1);
});

for (const platform of ['linux', 'darwin']) test(`${platform} keeps standard Go test execution`, t => {
  const f = fixture(t, () => '');
  runGoTests({ ...f.options, platform, packages: ['./...', 'shared/nethealth'] }, f.execute);
  assert.equal(f.calls.length, 1);
  assert.deepEqual(f.calls[0].args, ['test', '-mod=readonly', '-count=1', '-timeout=60s', './...', 'shared/nethealth']);
});
