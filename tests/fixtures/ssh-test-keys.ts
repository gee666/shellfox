// Test-only PPK encoder. Keys and complete key files are generated at runtime,
// independently of Shellfox's decoder, and never stored in the source tree.
import { createCipheriv, createHash, createHmac, createPrivateKey, createPublicKey, createECDH, randomBytes, type KeyObject } from 'node:crypto';
import { argon2id } from 'hash-wasm';
import { utils, type ParsedKey } from 'ssh2';

export type TestKeyKind='rsa'|'ecdsa256'|'ecdsa384'|'ecdsa521'|'ed25519';
export const TEST_KEY_PASSPHRASE='fixture-passphrase';
export interface TestKey {
  kind:TestKeyKind;
  keyObject:KeyObject;
  openssh?:Buffer;
  reference?:ParsedKey;
  algorithm:string;
  publicBlob:Buffer;
  privateBlob:Buffer;
}
const keys=new Map<TestKeyKind,TestKey>();
const files=new WeakMap<TestKey,Map<string,Promise<Buffer>>>();
const sshString=(data:Buffer|string)=>{const bytes=Buffer.from(data),length=Buffer.alloc(4);length.writeUInt32BE(bytes.length);return Buffer.concat([length,bytes]);};
function mpint(data:Buffer):Buffer {
  let first=0;while(first<data.length&&data[first]===0)first++;
  const unsigned=data.subarray(first);
  return sshString(unsigned.length&&(unsigned[0]&0x80)?Buffer.concat([Buffer.from([0]),unsigned]):unsigned);
}
const part=(value:string|undefined)=>{if(!value)throw new Error('Missing test key component');return Buffer.from(value,'base64url');};
export function testKeyFromObject(kind:TestKeyKind,keyObject:KeyObject):TestKey {
  const jwk=keyObject.export({format:'jwk'});let algorithm:string,publicBlob:Buffer,privateBlob:Buffer;
  if(kind==='rsa'){
    algorithm='ssh-rsa';publicBlob=Buffer.concat([sshString(algorithm),mpint(part(jwk.e)),mpint(part(jwk.n))]);
    privateBlob=Buffer.concat([mpint(part(jwk.d)),mpint(part(jwk.p)),mpint(part(jwk.q)),mpint(part(jwk.qi))]);
  }else if(kind==='ed25519'){
    algorithm='ssh-ed25519';publicBlob=Buffer.concat([sshString(algorithm),sshString(part(jwk.x))]);privateBlob=mpint(part(jwk.d));
  }else{
    const curves:Record<string,string>={'P-256':'nistp256','P-384':'nistp384','P-521':'nistp521'},curve=curves[jwk.crv!];
    if(!curve)throw new Error('Unsupported test curve');algorithm='ecdsa-sha2-'+curve;
    publicBlob=Buffer.concat([sshString(algorithm),sshString(curve),sshString(Buffer.concat([Buffer.from([4]),part(jwk.x),part(jwk.y)]))]);privateBlob=mpint(part(jwk.d));
  }
  return {kind,keyObject,algorithm,publicBlob,privateBlob};
}
export function generatedTestKey(kind:TestKeyKind):TestKey {
  const cached=keys.get(kind);if(cached)return cached;
  // ssh2 creates the independent OpenSSH reference. Node's KeyObject supplies
  // the mathematical components for the separate PuTTY-format encoder.
  const curveBits=kind==='ecdsa256'?256:kind==='ecdsa384'?384:521;
  const pair=kind==='rsa'?utils.generateKeyPairSync('rsa',{bits:2048,comment:'runtime-generated SSH test key'}):kind==='ed25519'?utils.generateKeyPairSync('ed25519',{comment:'runtime-generated SSH test key'}):utils.generateKeyPairSync('ecdsa',{bits:curveBits,comment:'runtime-generated SSH test key'});
  const reference=utils.parseKey(pair.private);if(reference instanceof Error||Array.isArray(reference))throw new Error('OpenSSH reference generation failed');
  const fixture={...testKeyFromObject(kind,createPrivateKey(reference.getPrivatePEM())),openssh:Buffer.from(pair.private),reference};keys.set(kind,fixture);return fixture;
}
export function ed25519SeedFixture(firstByte:number):TestKey {
  const seed=randomBytes(32);seed[0]=firstByte;
  // RFC 8410 PKCS8 prefix, followed by a fresh seed; no literal key material.
  const key=createPrivateKey({key:Buffer.concat([Buffer.from('302e020100300506032b657004220420','hex'),seed]),format:'der',type:'pkcs8'});
  return testKeyFromObject('ed25519',key);
}
export function shortEcdsa384Fixture():TestKey {
  const scalar=randomBytes(48);scalar[0]=0;scalar[1]|=0x80;
  const ec=createECDH('secp384r1');ec.setPrivateKey(scalar);const point=ec.getPublicKey(undefined,'uncompressed');
  return testKeyFromObject('ecdsa384',createPrivateKey({format:'jwk',key:{kty:'EC',crv:'P-384',d:scalar.toString('base64url'),x:point.subarray(1,49).toString('base64url'),y:point.subarray(49).toString('base64url')}}));
}
export interface PpkOptions {passphrase?:string;newline?:'\n'|'\r\n';comment?:string;extraPaddingBlock?:boolean}
function base64Lines(data:Buffer):string[]{return data.toString('base64').match(/.{1,64}/g)??[];}
async function encodePpk(key:TestKey,version:2|3,options:PpkOptions):Promise<Buffer>{
  const encrypted=options.passphrase!==undefined,cipher=encrypted?'aes256-cbc':'none',comment=options.comment??'runtime-generated SSH test key';
  let privatePlain:Buffer=key.privateBlob,cipherKey=Buffer.alloc(0),iv=Buffer.alloc(0),macKey:Buffer=Buffer.alloc(0);const kdf:string[]=[];
  if(encrypted){
    // PuTTY padding is arbitrary authenticated bytes, not PKCS7.
    const padding=(16-privatePlain.length%16)%16+(options.extraPaddingBlock?16:0);privatePlain=Buffer.concat([privatePlain,randomBytes(padding)]);
    if(version===2){
      const hashes=[0,1].map(sequence=>{const prefix=Buffer.alloc(4);prefix.writeUInt32BE(sequence);return createHash('sha1').update(prefix).update(options.passphrase!).digest();});
      cipherKey=Buffer.concat(hashes).subarray(0,32);iv=Buffer.alloc(16);
    }else{
      const salt=randomBytes(16),memory=32,passes=1,parallelism=1;
      const derived=Buffer.from(await argon2id({password:options.passphrase!,salt,memorySize:memory,iterations:passes,parallelism,hashLength:80,outputType:'binary'}));
      cipherKey=derived.subarray(0,32);iv=derived.subarray(32,48);macKey=derived.subarray(48);
      kdf.push('Key-Derivation: Argon2id','Argon2-Memory: '+memory,'Argon2-Passes: '+passes,'Argon2-Parallelism: '+parallelism,'Argon2-Salt: '+salt.toString('hex'));
    }
  }
  if(version===2)macKey=createHash('sha1').update('putty-private-key-file-mac-key').update(encrypted?options.passphrase!:'').digest();
  const macData=Buffer.concat([sshString(key.algorithm),sshString(cipher),sshString(comment),sshString(key.publicBlob),sshString(privatePlain)]);
  const mac=createHmac(version===2?'sha1':'sha256',macKey).update(macData).digest('hex');
  let privateStored=privatePlain;
  if(encrypted){const aes=createCipheriv('aes-256-cbc',cipherKey,iv);aes.setAutoPadding(false);privateStored=Buffer.concat([aes.update(privatePlain),aes.final()]);}
  const pub=base64Lines(key.publicBlob),priv=base64Lines(privateStored);
  const lines=['PuTTY-User-Key-File-'+version+': '+key.algorithm,'Encryption: '+cipher,'Comment: '+comment,'Public-Lines: '+pub.length,...pub,...kdf,'Private-Lines: '+priv.length,...priv,'Private-MAC: '+mac];
  return Buffer.from(lines.join(options.newline??'\n')+(options.newline??'\n'),'utf8');
}
export function generatedPpk(key:TestKey,version:2|3,options:PpkOptions={}):Promise<Buffer>{
  let cache=files.get(key);if(!cache){cache=new Map();files.set(key,cache);}
  const id=JSON.stringify({version,...options});let file=cache.get(id);
  if(!file){file=encodePpk(key,version,options);cache.set(id,file);}return file;
}
export function testPublicKey(key:TestKey):KeyObject{return createPublicKey(key.keyObject);}
