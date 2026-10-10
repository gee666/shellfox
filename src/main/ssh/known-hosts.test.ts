import { expect, it } from 'vitest';
import { mkdir, mkdtemp, rm, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { generateKeyPairSync } from 'node:crypto';
import { utils } from 'ssh2';
import { trustHost, hostKeyInfo } from './known-hosts';
const key=()=>{const parsed=utils.parseKey(generateKeyPairSync('rsa',{modulusLength:2048}).privateKey.export({type:'pkcs1',format:'pem'}).toString());if(parsed instanceof Error||Array.isArray(parsed))throw new Error('key fixture');return parsed.getPublicSSH();};
it('requires explicit TOFU, reuses exact keys, refuses changed keys and never wipes corrupt trust',async()=>{
  await mkdir('tmp',{recursive:true});const directory=await mkdtemp(path.resolve('tmp/ssh-hosts-'));
  try {
    const first=key();expect(hostKeyInfo(first)).toMatchObject({type:'ssh-rsa',fingerprint:expect.stringMatching(/^SHA256:/)});
    expect(await trustHost(directory,'host',22,first,async()=>false)).toBe(false);
    expect(await trustHost(directory,'host',22,first,async()=>true)).toBe(true);
    expect(await trustHost(directory,'HOST',22,first,async()=>{throw new Error('must not ask');})).toBe(true);
    const file=path.join(directory,'ssh-known-hosts.json'),previous=await readFile(file,'utf8');
    await expect(trustHost(directory,'host',22,key(),async()=>true)).rejects.toThrow(/host key changed/);expect(await readFile(file,'utf8')).toBe(previous);
    await writeFile(file,'broken');await expect(trustHost(directory,'host',22,first,async()=>true)).rejects.toThrow(/corrupt/);expect(await readFile(file,'utf8')).toBe('broken');
  }finally{await rm(directory,{recursive:true,force:true});}
});
it('merges concurrent independent trust decisions under a bounded file lock',async()=>{
  const directory=await mkdtemp(path.resolve('tmp/ssh-hosts-'));
  try { const publicKey=key();await Promise.all(['a','b'].map(host=>trustHost(directory,host,22,publicKey,async()=>true)));expect(Object.keys(JSON.parse(await readFile(path.join(directory,'ssh-known-hosts.json'),'utf8')).hosts)).toHaveLength(2); }
  finally{await rm(directory,{recursive:true,force:true});}
});
