import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { afterEach, beforeEach, expect, it } from 'vitest';
import { startHandoff } from './handoff';
let root: string, helper: string;
beforeEach(async () => {
  await mkdir('tmp', { recursive: true }); root = await mkdtemp(path.resolve('tmp/update-handoff-')); helper = path.join(root, 'helper.cjs');
  await writeFile(helper, `const fs=require('node:fs'),path=require('node:path');
const dir=process.argv[2],has=n=>fs.existsSync(path.join(dir,n)),put=n=>fs.writeFileSync(path.join(dir,n),'');
if(process.argv[3]==='crash')process.exit(2);
if(process.argv[3]!=='silent')put('ready');
let committed=false;
setInterval(()=>{if(has('cancel'))process.exit(0);
if(has('commit')&&process.argv[3]!=='no-ack'){committed=true;put('committed');}
if(has('manager-exited')){if(committed)put('installed');process.exit(0);}},10);
setTimeout(()=>process.exit(0),3000);
`);
});
afterEach(async () => { await rm(root, { recursive: true, force: true }); });
it('acknowledges startup and commit without installing while the manager remains alive', async () => {
  const lease = await startHandoff(process.execPath, [helper, root], root, 1000);
  expect(lease.committed).toBe(false); await expect(readFile(path.join(root, 'installed'))).rejects.toThrow();
  await lease.commit(); expect(lease.committed).toBe(true);
  await expect(readFile(path.join(root, 'installed'))).rejects.toThrow();
  await writeFile(path.join(root, 'manager-exited'), ''); await expect.poll(async () => readFile(path.join(root, 'installed'), 'utf8')).toBe('');
});
it('cancels a ready helper before commit so subsequent normal quit cannot install', async () => {
  const lease = await startHandoff(process.execPath, [helper, root], root, 1000);
  await Promise.all([lease.cancel(), lease.cancel()]);
  await writeFile(path.join(root, 'manager-exited'), ''); await expect(lease.commit()).rejects.toThrow();
  await expect(readFile(path.join(root, 'installed'))).rejects.toThrow();
});
it('detects early script failure rather than accepting the spawn event', async () => {
  await expect(startHandoff(process.execPath, [helper, root, 'crash'], root, 1000)).rejects.toThrow('exited');
});
it('rejects missing executables and readiness timeouts and terminates the pending helper', async () => {
  await expect(startHandoff(path.join(root, 'missing-executable'), [], root, 100)).rejects.toThrow();
  const control = await mkdtemp(path.join(root, 'retry-'));
  await expect(startHandoff(process.execPath, [helper, control, 'silent'], control, 100)).rejects.toThrow('acknowledge');
  await expect(readFile(path.join(root, 'committed'))).rejects.toThrow();
});
it('rejects a lost commit acknowledgement and cancels even when the commit marker exists', async () => {
  const lease = await startHandoff(process.execPath, [helper, root, 'no-ack'], root, 1000);
  await expect(lease.commit()).rejects.toThrow('acknowledge'); await lease.cancel();
  await writeFile(path.join(root, 'manager-exited'), ''); await expect(readFile(path.join(root, 'installed'))).rejects.toThrow();
});
