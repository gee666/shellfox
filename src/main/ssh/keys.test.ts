import { expect, it } from 'vitest';
import { verify } from 'node:crypto';
import { utils } from 'ssh2';
import { parsePrivateKey } from './keys';
import { generatedTestKey, generatedPpk, ed25519SeedFixture, shortEcdsa384Fixture, testPublicKey, TEST_KEY_PASSPHRASE, type TestKeyKind } from '../../../tests/fixtures/ssh-test-keys';
const kinds:TestKeyKind[]=['rsa','ecdsa256','ecdsa384','ecdsa521','ed25519'];
it.each(kinds)('reads generated %s OpenSSH and encrypted/unencrypted PPK v2/v3 and verifies signatures',async kind=>{
 const fixture=generatedTestKey(kind),reference=utils.parseKey(fixture.openssh!),message=Buffer.from('fixture signature');
 if(reference instanceof Error||Array.isArray(reference))throw new Error('reference key');
 const expected=await parsePrivateKey(fixture.openssh!);expect(expected.getPublicSSH()).toEqual(reference.getPublicSSH());
 for(const version of [2,3] as const)for(const encrypted of [false,true]){
  const data=await generatedPpk(fixture,version,encrypted?{passphrase:TEST_KEY_PASSPHRASE}:{});
  if(encrypted){await expect(parsePrivateKey(data)).rejects.toThrow(/passphrase/);await expect(parsePrivateKey(data,'wrong')).rejects.toThrow(/integrity/);}
  const key=await parsePrivateKey(data,encrypted?TEST_KEY_PASSPHRASE:undefined);expect(key.getPublicSSH()).toEqual(reference.getPublicSSH());
  const signature=key.sign(message);expect(signature).not.toBeInstanceOf(Error);expect(reference.verify(message,signature as Buffer)).toBe(true);
 }
});
it('accepts PuTTY-style CRLF, wrapped lines, UTF-8 comments and authenticated non-PKCS7 padding',async()=>{
 const fixture=generatedTestKey('ecdsa384'),message=Buffer.from('padded key');
 for(const version of [2,3] as const){
  const data=await generatedPpk(fixture,version,{passphrase:TEST_KEY_PASSPHRASE,newline:'\r\n',comment:'generated test key: clé / ключ',extraPaddingBlock:true});
  const key=await parsePrivateKey(data,TEST_KEY_PASSPHRASE);expect(key.getPublicSSH()).toEqual(fixture.reference!.getPublicSSH());
  expect(fixture.reference!.verify(message,key.sign(message) as Buffer)).toBe(true);
 }
});
it.each([0,0x80,0xff])('restores Ed25519 mpint seed width and sign padding, first byte=%s',async firstByte=>{
 const fixture=ed25519SeedFixture(firstByte),message=Buffer.from('mpint seed');
 for(const version of [2,3] as const){
  const key=await parsePrivateKey(await generatedPpk(fixture,version));expect(key.getPublicSSH()).toEqual(fixture.publicBlob);
  expect(verify(null,message,testPublicKey(fixture),key.sign(message) as Buffer)).toBe(true);
 }
});
it('left-pads a short nistp384 scalar from PuTTY mpint form',async()=>{
 const fixture=shortEcdsa384Fixture(),message=Buffer.from('short curve scalar');
 for(const version of [2,3] as const){
  const key=await parsePrivateKey(await generatedPpk(fixture,version));expect(key.getPublicSSH()).toEqual(fixture.publicBlob);
  expect(verify('sha384',message,testPublicKey(fixture),key.sign(message) as Buffer)).toBe(true);
 }
});
it.each(kinds)('converts generated %s PKCS8 keys, including encrypted PKCS8',async kind=>{
 const fixture=generatedTestKey(kind),message=Buffer.from('pkcs8 fixture');
 for(const encrypted of [false,true]){
  const data=Buffer.from(fixture.keyObject.export({type:'pkcs8',format:'pem',...(encrypted?{cipher:'aes-256-cbc',passphrase:TEST_KEY_PASSPHRASE}:{})}));
  if(encrypted)await expect(parsePrivateKey(data)).rejects.toThrow(/passphrase/);
  const key=await parsePrivateKey(data,encrypted?TEST_KEY_PASSPHRASE:undefined);expect(key.getPublicSSH()).toEqual(fixture.reference!.getPublicSSH());
  expect(fixture.reference!.verify(message,key.sign(message) as Buffer)).toBe(true);
 }
});
it('detects unauthenticated PPK comments/data and unsupported input',async()=>{
 const data=await generatedPpk(generatedTestKey('rsa'),3);
 await expect(parsePrivateKey(Buffer.from(data.toString().replace('Comment: runtime-generated SSH test key','Comment: tampered test key')))).rejects.toThrow(/integrity/);
 await expect(parsePrivateKey(Buffer.from('broken'))).rejects.toThrow();
});
