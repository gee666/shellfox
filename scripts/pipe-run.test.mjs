import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { afterEach, beforeEach, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const helper = path.join(root, 'tmp/terminal-native', `win-${process.arch}`, 'Shellfox.ElevatedTerminal.exe');
const supported = process.platform === 'win32' && existsSync(helper);
let work;
beforeEach(() => { mkdirSync(path.join(root, 'tmp'), { recursive: true }); work = mkdtempSync(path.join(root, 'tmp/pipe-run-test-')); });
afterEach(() => { rmSync(work, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 }); });
function config(args, env = process.env) {
  const file = path.join(work, 'launch.json');
  writeFileSync(file, JSON.stringify({ executable: process.execPath, args, cwd: work, env }), { mode: 0o600 });
  return file;
}
function alive(pid) { try { process.kill(pid, 0); return true; } catch { return false; } }
async function waitFor(condition) {
  for (let i = 0; i < 300; i++) { if (condition()) return; await delay(20); }
  throw new Error('Timed out waiting for native process containment.');
}

test('pipe-run preserves stdio, argv, cwd, environment and the target exit code without a protocol', { skip: !supported, timeout: 15000 }, () => {
  const args = ['', 'spaces 雪🙂', 'quote"inside', 'slashes\\', 'before\\"after', '& | > %PATH%'];
  const file = config(['-e', `let input='';process.stdin.setEncoding('utf8');process.stdin.on('data',x=>input+=x);process.stdin.on('end',()=>{process.stdout.write(JSON.stringify({args:process.argv.slice(1),input,cwd:process.cwd(),env:process.env.PIPE_RUN_TEST}));process.stderr.write('target stderr\\n');process.exitCode=23;});`, ...args], { ...process.env, PIPE_RUN_TEST: 'kept=a=b' });
  const result = spawnSync(helper, ['pipe-run', file], { input: 'raw stdin 雪\n', encoding: 'utf8', timeout: 10000, windowsHide: true });
  assert.ifError(result.error);
  assert.equal(result.status, 23, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout), { args, input: 'raw stdin 雪\n', cwd: work, env: 'kept=a=b' });
  assert.equal(result.stderr, 'target stderr\n');
  assert.equal(existsSync(file), false);
});

test('pipe-run deletes malformed configuration and fails only on stderr', { skip: !supported, timeout: 15000 }, () => {
  const file = path.join(work, 'bad.json'); writeFileSync(file, '{"secret":"must not echo"}');
  const result = spawnSync(helper, ['pipe-run', file], { encoding: 'utf8', timeout: 10000, windowsHide: true });
  assert.notEqual(result.status, 0);
  assert.equal(result.stdout, '');
  assert.match(result.stderr, /Shellfox pipe-run failed/);
  assert.doesNotMatch(result.stderr, /must not echo/);
  assert.equal(existsSync(file), false);
});

test('pipe-run reports an invalid invocation without stdout', { skip: !supported, timeout: 15000 }, () => {
  const result = spawnSync(helper, ['pipe-run'], { encoding: 'utf8', timeout: 10000, windowsHide: true });
  assert.notEqual(result.status, 0); assert.equal(result.stdout, '');
  assert.match(result.stderr, /Shellfox pipe-run failed/);
});

for (const abrupt of [false, true]) {
  test(`pipe-run kills descendants after ${abrupt ? 'abrupt helper termination' : 'natural root exit'}`, { skip: !supported, timeout: 20000 }, async () => {
    const marker = path.join(work, 'descendant.pid');
    const file = config(['-e', `const {spawn}=require('node:child_process');const child=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{detached:true,stdio:'ignore'});require('node:fs').writeFileSync(${JSON.stringify(marker)},String(child.pid));child.unref();${abrupt ? 'setInterval(()=>{},1000);' : 'process.exitCode=17;'}`]);
    const child = spawn(helper, ['pipe-run', file], { windowsHide: true });
    let stderr = ''; child.stdout.resume(); child.stderr.setEncoding('utf8'); child.stderr.on('data', text => { stderr += text; });
    const exited = new Promise((resolve, reject) => { child.once('error', reject); child.once('exit', code => resolve(code)); });
    let descendant;
    try {
      await waitFor(() => existsSync(marker));
      descendant = Number(readFileSync(marker, 'utf8'));
      assert.equal(existsSync(file), false);
      if (abrupt) { assert.equal(alive(descendant), true); child.kill(); }
      const code = await exited;
      if (!abrupt) assert.equal(code, 17, stderr);
      await waitFor(() => !alive(descendant));
      assert.equal(stderr, '');
    } finally {
      if (child.exitCode === null) child.kill();
      await exited.catch(() => {});
      if (descendant) await waitFor(() => !alive(descendant));
    }
  });
}
