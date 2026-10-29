import { describe, expect, it } from 'vitest';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { readFileSync } from 'node:fs';
import { setTimeout as delay } from 'node:timers/promises';
import { AutomationRuns } from './automation';

const env: Record<string, string> = {};
for (const [key, value] of Object.entries(process.env)) if (value !== undefined) env[key] = value;
async function descendant(stdio: 'inherit' | 'ignore') {
  const manager = new AutomationRuns({ focusSession: () => ({ ok: false, error: { code: 'UNSUPPORTED', message: 'No viewer', retryable: false } }) }, path.resolve('tmp'));
  let descendantPid = 0;
  try {
    const run = await manager.start({ requestId: randomUUID(), executable: process.execPath, args: ['-e', `const {spawn}=require('node:child_process');const child=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:${JSON.stringify(stdio)}});process.stdout.write(String(child.pid));child.unref();`], cwd: process.cwd(), env, detached: false }, 'owner');
    let after = 0, output = '', exited = false;
    for (let i = 0; i < 500; i++) {
      const result = manager.read(run.id, after, 'owner');
      for (const frame of result.frames) { output += Buffer.from(frame.data, 'base64').toString(); after = frame.sequence; }
      if (!result.frames.length && result.run.state === 'exited') { expect(result.run.exitCode).toBe(0); exited = true; break; }
      await delay(10);
    }
    descendantPid = Number(output);
    expect(exited).toBe(true); expect(descendantPid).toBeGreaterThan(0);
    for (let i = 0; i < 100; i++) {
      try {
        process.kill(descendantPid, 0);
        if (process.platform === 'linux') {
          const stat = readFileSync(`/proc/${descendantPid}/stat`, 'utf8');
          if (stat.slice(stat.lastIndexOf(')') + 2).startsWith('Z')) { descendantPid = 0; break; }
        }
      } catch { descendantPid = 0; break; }
      await delay(10);
    }
    expect(descendantPid).toBe(0);
  } finally {
    await manager.close();
    if (descendantPid) { try { process.kill(descendantPid, 'SIGKILL'); } catch { /* Already stopped. */ } }
  }
}
describe('automation process ownership', () => {
  it('cleans descendants that inherit output when the command root exits', () => descendant('inherit'), 15000);
  it('cleans descendants that redirect output when the command root exits', () => descendant('ignore'), 15000);
});
