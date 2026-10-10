import path from 'node:path';
import { readFile, open, unlink } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { z } from 'zod';
import { atomicJson } from './storage';
const schema = z.object({ version: z.literal(1), hosts: z.record(z.string(), z.object({ fingerprint: z.string(), type: z.string() }).strict()) }).strict();
export function hostKeyInfo(key: Buffer) {
  if(key.length<4)throw new Error('invalid host key');
  const length = key.readUInt32BE(0), type=key.subarray(4,4+length).toString();
  if(length<1||length>200||length>key.length-4||!/^[a-zA-Z0-9@._+-]+$/.test(type))throw new Error('invalid host key type');
  return { type, fingerprint: 'SHA256:' + createHash('sha256').update(key).digest('base64').replace(/=+$/,'') };
}
export async function trustHost(userData: string, host: string, port: number, key: Buffer, ask: (info: ReturnType<typeof hostKeyInfo>) => Promise<boolean>): Promise<boolean> {
  const file = path.join(userData,'ssh-known-hosts.json'), id = JSON.stringify([host.toLowerCase(),port]), info = hostKeyInfo(key);
  const read = async () => {
    try { return schema.parse(JSON.parse(await readFile(file,'utf8'))); }
    catch (e) { if ((e as NodeJS.ErrnoException).code === 'ENOENT') return { version: 1 as const, hosts: {} as Record<string, typeof info> }; throw new Error('ssh-known-hosts.json is corrupt; repair it before connecting.'); }
  };
  const changed = () => new Error(`host key changed for ${host}:${port}; refusing connection. Remove this host from ${file} only after verifying the new key.`);
  const saved = (await read()).hosts[id];
  if (saved) { if (saved.fingerprint !== info.fingerprint || saved.type !== info.type) throw changed(); return true; }
  if (!await ask(info)) return false;
  // Separate helpers can trust hosts concurrently. Lock the read-modify-write,
  // never the interactive question, and recheck after acquiring the lock.
  let lock;
  for (let i=0;i<50;i++) {
    try { lock = await open(file+'.lock','wx',0o600); break; }
    catch (e) { if ((e as NodeJS.ErrnoException).code !== 'EEXIST') throw e; await new Promise(r=>setTimeout(r,100)); }
  }
  if (!lock) throw new Error('known hosts are busy; retry or remove the stale .lock file.');
  try {
    const latest = await read(), previous = latest.hosts[id];
    if (previous && (previous.fingerprint !== info.fingerprint || previous.type !== info.type)) throw changed();
    latest.hosts[id] = info; await atomicJson(file,latest); return true;
  } finally { await lock.close(); await unlink(file+'.lock'); }
}
