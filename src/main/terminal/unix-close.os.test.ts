import { it, expect } from 'vitest';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import path from 'node:path';
import { UNIX_TREE_CLOSE_SCRIPT, PIDFD_PREFLIGHT, PIDFD_READY } from './unix-close';
import { UNIX_CLOSE_NATIVE_FIXTURE } from './unix-close.native-fixture';
import { UNIX_SAFETY_NATIVE_FIXTURE } from './unix-close.safety-fixture';
import { decodeWsl } from './profiles';
// OWNED INTEGRATION HOLD: this file is never selected by default unit.
it.runIf(process.env.SHELLFOX_UNIX_TREE_TEST === '1')('actual Linux/WSL kernel closes HUP-ignoring background jobs and protects unrelated/reused identities', async () => {
  const run = promisify(execFile);
  let file = '/usr/bin/python3', prefix: string[] = [];
  if (process.platform === 'win32') {
    file = path.win32.join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'wsl.exe');
    const list = await run(file, ['--list', '--quiet'], { encoding: 'buffer', timeout: 8000, maxBuffer: 1024 * 1024 });
    const distros = decodeWsl(list.stdout).trim().split(/\r?\n/).filter(Boolean), distro = process.env.SHELLFOX_UNIX_TREE_DISTRO ?? distros.find(d => /Debian/i.test(d)) ?? distros[0];
    if (!distro) throw new Error('No guest for actual kernel acceptance.');
    prefix = ['--distribution', distro, '--exec', 'python3'];
  } else if (process.platform !== 'linux') throw new Error('This acceptance requires actual Linux pidfds.');
  const preflight = await run(file, [...prefix, '-c', PIDFD_PREFLIGHT], { encoding: 'utf8', timeout: 8000, maxBuffer: 1024 * 1024 }); expect(preflight.stdout.trim()).toBe(PIDFD_READY);
  const result = await run(file, [...prefix, '-c', UNIX_CLOSE_NATIVE_FIXTURE, UNIX_TREE_CLOSE_SCRIPT], { encoding: 'utf8', timeout: 30000, maxBuffer: 1024 * 1024 });
  const checked = JSON.parse(result.stdout); expect(checked.ok).toBe(true); expect(checked.assertions).toHaveLength(5);
  // Both SID reuse and allocator mutation are confined to a private PID/mount namespace.
  const namespaceFile = process.platform === 'win32' ? file : '/usr/bin/unshare';
  const namespacePrefix = process.platform === 'win32' ? [...prefix.slice(0, -1), 'unshare'] : [];
  const safety = await run(namespaceFile, [...namespacePrefix, '--user', '--map-root-user', '--pid', '--fork', '--mount-proc', 'python3', '-c', UNIX_SAFETY_NATIVE_FIXTURE, UNIX_TREE_CLOSE_SCRIPT], { encoding: 'utf8', timeout: 30000, maxBuffer: 1024 * 1024 });
  const proof = JSON.parse(safety.stdout);
  expect(proof.sid_reuse).toMatchObject({ unrelated_new_session_killed: false, proven_child_killed: false }); expect(proof.sid_reuse.helper_exit).not.toBe(0);
  expect(proof.post_commit_alarm).toMatchObject({ root_alive: true, child_alive: true, unrelated_alive: true });
  expect(proof.post_commit_alarm.root_state).not.toBe('T'); expect(proof.post_commit_alarm.child_state).not.toBe('T');
}, 40000);
