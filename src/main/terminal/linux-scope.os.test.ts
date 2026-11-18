import { execFile, spawn } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { promisify } from 'node:util';
import { expect, it } from 'vitest';
import { probeTerminalScope, raiseOomScore, scopedCommand, TERMINAL_OOM_SCORE_ADJ, waitForScopedExec } from './linux-scope';

const run = promisify(execFile);
const runner = process.platform === 'linux' ? await probeTerminalScope() : null;
it.skipIf(process.platform !== 'linux')('raises a process OOM score but never lowers it', async () => {
    const child = spawn('sleep', ['30']);
    try {
      const score = async () => Number(await readFile(`/proc/${child.pid}/oom_score_adj`, 'utf8'));
      // CI children may inherit a score higher than our default from the runner.
      const inherited = await score();
      await raiseOomScore(child.pid!);
      const baseline = Math.max(inherited, TERMINAL_OOM_SCORE_ADJ);
      expect(await score()).toBe(baseline);
      const higher = Math.min(1000, baseline + 100);
      await raiseOomScore(child.pid!, higher);
      expect(await score()).toBe(higher);
      await raiseOomScore(child.pid!, 100);
      expect(await score()).toBe(higher);
    } finally { child.kill(); }
  });
it.skipIf(!runner)('runs the shell as the same PID in its own OOM-tolerant scope', async () => {
    const command = scopedCommand(runner!, { file: '/bin/sh', args: ['-c', 'cat /proc/self/cgroup; exec sleep 30'] });
    const child = spawn(command.file, command.args, { stdio: ['ignore', 'pipe', 'inherit'] });
    try {
      const cgroup = await new Promise<string>(resolve => child.stdout.once('data', data => resolve(String(data))));
      const unit = /\/(shellfox-terminal-[0-9a-f-]{36}\.scope)$/m.exec(cgroup.trim())?.[1];
      expect(unit).toBeDefined();
      await waitForScopedExec(child.pid!, runner!);
      const { stdout } = await run('systemctl', ['--user', 'show', unit!, '-p', 'OOMPolicy', '--value']);
      expect(stdout).toContain('continue');
      expect(path.basename((await run('readlink', [`/proc/${child.pid}/exe`])).stdout.trim())).toBe('sleep');
    } finally { child.kill(); }
  });
