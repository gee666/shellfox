import { expect, it } from 'vitest';
import { readFile } from 'node:fs/promises';
import { generateKeyPairSync } from 'node:crypto';
import { parsePrivateKey } from './keys';
const fixture=(name:string)=>readFile('tests/fixtures/ssh-keys/'+name);
it.each(['rsa','ecdsa','ed25519'])('reads real PuTTYgen %s v2 and Argon2-encrypted v3 fixtures and checks signatures',async type=>{
  const expected=await parsePrivateKey(await fixture(type+'.openssh')),message=Buffer.from('fixture signature');
  for(const version of [2,3]){
    const data=await fixture(type+'-v'+version+'.ppk');
    await expect(parsePrivateKey(data)).rejects.toThrow(/passphrase/);
    await expect(parsePrivateKey(data,'wrong')).rejects.toThrow(/integrity/);
    const key=await parsePrivateKey(data,'fixture-passphrase');expect(key.getPublicSSH()).toEqual(expected.getPublicSSH());
    const signature=key.sign(message);expect(signature).not.toBeInstanceOf(Error);
    expect(expected.verify(message,signature as Buffer)).toBe(true);
  }
});
it('reads encrypted PKCS8 keys and rejects unsupported/corrupt input',async()=>{
  const pair=generateKeyPairSync('ed25519'),data=Buffer.from(pair.privateKey.export({type:'pkcs8',format:'pem',cipher:'aes-256-cbc',passphrase:'fixture-passphrase'}));
  await expect(parsePrivateKey(data)).rejects.toThrow(/passphrase/);
  const key=await parsePrivateKey(data,'fixture-passphrase');expect(key.type).toBe('ssh-ed25519');
  await expect(parsePrivateKey(Buffer.from('broken'))).rejects.toThrow();
});
