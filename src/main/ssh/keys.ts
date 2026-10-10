import { createPrivateKey, createHash, createHmac, createDecipheriv, timingSafeEqual } from 'node:crypto';
import { argon2id, argon2i, argon2d } from 'hash-wasm';
import { utils, type ParsedKey } from 'ssh2';
class Reader {
  private offset = 0;
  constructor(private data: Buffer) {}
  string(): Buffer {
    if(this.offset+4>this.data.length)throw new Error('malformed PuTTY key');
    const size=this.data.readUInt32BE(this.offset);this.offset+=4;
    if(size>this.data.length-this.offset)throw new Error('malformed PuTTY key');
    const value=this.data.subarray(this.offset,this.offset+size);this.offset+=size;return value;
  }
}
const strip = (b: Buffer) => { while(b.length>1&&b[0]===0)b=b.subarray(1);return b; };
const b64 = (b: Buffer) => strip(b).toString('base64url');
const number = (b: Buffer) => BigInt('0x'+(strip(b).toString('hex')||'0'));
const bytes = (n: bigint) => { let s=n.toString(16);if(s.length%2)s='0'+s;return Buffer.from(s,'hex'); };
const sshString = (b: Buffer|string) => { const data=Buffer.from(b);const size=Buffer.alloc(4);size.writeUInt32BE(data.length);return Buffer.concat([size,data]); };
function exportForSsh(key: import('node:crypto').KeyObject): string {
  if(key.asymmetricKeyType==='rsa')return key.export({type:'pkcs1',format:'pem'}).toString();
  if(key.asymmetricKeyType==='ec')return key.export({type:'sec1',format:'pem'}).toString();
  if(key.asymmetricKeyType==='ed25519'){
    const jwk=key.export({format:'jwk'}),pub=Buffer.from(jwk.x!,'base64url'),seed=Buffer.from(jwk.d!,'base64url'),type='ssh-ed25519';
    const publicBlob=Buffer.concat([sshString(type),sshString(pub)]),checks=Buffer.alloc(8);
    let privateBlob=Buffer.concat([checks,sshString(type),sshString(pub),sshString(Buffer.concat([seed,pub])),sshString('')]);
    const padding=Buffer.from(Array.from({length:8-privateBlob.length%8},(_,i)=>i+1));privateBlob=Buffer.concat([privateBlob,padding]);
    const count=Buffer.from([0,0,0,1]),blob=Buffer.concat([Buffer.from('openssh-key-v1\0'),sshString('none'),sshString('none'),sshString(''),count,sshString(publicBlob),sshString(privateBlob)]);
    return '-----BEGIN OPENSSH PRIVATE KEY-----\n'+blob.toString('base64').match(/.{1,70}/g)!.join('\n')+'\n-----END OPENSSH PRIVATE KEY-----\n';
  }
  throw new Error('unsupported private key type');
}
/** ssh2 only understands RSA/DSA PPK v2. Decode v3, authenticate it, and
 * convert in memory. Never write a decrypted key to disk. */
async function ppk3(text: string, password?: string): Promise<string> {
  const lines=text.trim().split(/\r?\n/), fields: Record<string,string> = {};
  let publicBlob=Buffer.alloc(0), privateBlob=Buffer.alloc(0);
  for(let i=0;i<lines.length;i++){
    const split=lines[i].indexOf(': ');if(split<0)continue;
    const name=lines[i].slice(0,split),value=lines[i].slice(split+2);fields[name]=value;
    if(name==='Public-Lines'||name==='Private-Lines'){
      const count=Number(value);if(!Number.isInteger(count)||count<1||count>10000||i+count>=lines.length)throw new Error('malformed PuTTY key');
      const blob=Buffer.from(lines.slice(i+1,i+count+1).join(''),'base64');i+=count;
      if(name==='Public-Lines')publicBlob=blob;else privateBlob=blob;
    }
  }
  const version = fields['PuTTY-User-Key-File-3'] ? 3 : 2;
  const type=fields['PuTTY-User-Key-File-'+version], encryption=fields.Encryption;
  let macKey: Buffer=Buffer.alloc(0);
  if(encryption!=='none'){
    if(password===undefined)throw new Error('encrypted PuTTY key requires a passphrase');
    if(encryption!=='aes256-cbc')throw new Error('unsupported PuTTY encryption');
    if(version===2){
      const one=Buffer.from([0,0,0,0]),two=Buffer.from([0,0,0,1]);
      const key=Buffer.concat([createHash('sha1').update(one).update(password).digest(),createHash('sha1').update(two).update(password).digest()]).subarray(0,32);
      const decipher=createDecipheriv('aes-256-cbc',key,Buffer.alloc(16));decipher.setAutoPadding(false);
      privateBlob=Buffer.concat([decipher.update(privateBlob),decipher.final()]);
    }else {
    const variants: Record<string,typeof argon2id> = {'Argon2id':argon2id,'Argon2i':argon2i,'Argon2d':argon2d}, fn=variants[fields['Key-Derivation']];
    const memory=Number(fields['Argon2-Memory']),iterations=Number(fields['Argon2-Passes']),parallelism=Number(fields['Argon2-Parallelism']);
    if(!fn||!Number.isInteger(memory)||memory<8||memory>262144||!Number.isInteger(iterations)||iterations<1||iterations>100||!Number.isInteger(parallelism)||parallelism<1||parallelism>64)throw new Error('unsupported PuTTY key derivation parameters');
    const derived=Buffer.from(await fn({ password, salt:Buffer.from(fields['Argon2-Salt'],'hex'), parallelism, iterations, memorySize:memory, hashLength:80, outputType:'binary' }));
    const decipher=createDecipheriv('aes-256-cbc',derived.subarray(0,32),derived.subarray(32,48));decipher.setAutoPadding(false);
    privateBlob=Buffer.concat([decipher.update(privateBlob),decipher.final()]);macKey=derived.subarray(48);
    }
  }
  if(version===2)macKey=createHash('sha1').update('putty-private-key-file-mac-key').update(password??'').digest();
  const actual=createHmac(version===3?'sha256':'sha1',macKey).update(Buffer.concat([sshString(type),sshString(encryption),sshString(fields.Comment??''),sshString(publicBlob),sshString(privateBlob)])).digest(), expected=Buffer.from(fields['Private-MAC']??'','hex');
  if(actual.length!==expected.length||!timingSafeEqual(actual,expected))throw new Error('PuTTY key integrity check failed; wrong passphrase?');
  const pub=new Reader(publicBlob),priv=new Reader(privateBlob);
  if(pub.string().toString()!==type)throw new Error('malformed PuTTY key type');
  let jwk: import('node:crypto').JsonWebKey;
  if(type==='ssh-rsa'){
    const e=pub.string(),n=pub.string(),d=priv.string(),p=priv.string(),q=priv.string(),qi=priv.string();
    jwk={ kty:'RSA',e:b64(e),n:b64(n),d:b64(d),p:b64(p),q:b64(q),qi:b64(qi),dp:b64(bytes(number(d)%(number(p)-1n))),dq:b64(bytes(number(d)%(number(q)-1n))) };
  }else if(type==='ssh-ed25519'){
    const publicKey=pub.string(),privateKey=strip(priv.string());
    if(privateKey.length>32)throw new Error('malformed PuTTY Ed25519 key');
    jwk={kty:'OKP',crv:'Ed25519',x:publicKey.toString('base64url'),d:Buffer.concat([Buffer.alloc(32-privateKey.length),privateKey]).toString('base64url')};
  }else if(type?.startsWith('ecdsa-sha2-')){
    const curve=pub.string().toString(),point=pub.string(),size=(point.length-1)/2, curves:Record<string,string>={nistp256:'P-256',nistp384:'P-384',nistp521:'P-521'};
    if(!curves[curve]||point[0]!==4||!Number.isInteger(size))throw new Error('unsupported PuTTY elliptic curve');
    const d=strip(priv.string());if(d.length>size)throw new Error('malformed PuTTY private scalar');
    jwk={kty:'EC',crv:curves[curve],x:point.subarray(1,1+size).toString('base64url'),y:point.subarray(1+size).toString('base64url'),d:Buffer.concat([Buffer.alloc(size-d.length),d]).toString('base64url')};
  }else if(type==='ssh-dss'){
    const values=[Buffer.from([0]),pub.string(),pub.string(),pub.string(),pub.string(),priv.string()];
    const length=(size:number)=>size<128?Buffer.from([size]):(()=>{const b=bytes(BigInt(size));return Buffer.concat([Buffer.from([0x80+b.length]),b]);})();
    const integers=values.map(value=>{let data=strip(value);if(data[0]&0x80)data=Buffer.concat([Buffer.from([0]),data]);return Buffer.concat([Buffer.from([2]),length(data.length),data]);});
    const data=Buffer.concat(integers),der=Buffer.concat([Buffer.from([0x30]),length(data.length),data]);
    return '-----BEGIN DSA PRIVATE KEY-----\n'+der.toString('base64').match(/.{1,64}/g)!.join('\n')+'\n-----END DSA PRIVATE KEY-----\n';
  }else throw new Error('unsupported PuTTY key type: '+type);
  return exportForSsh(createPrivateKey({key:jwk,format:'jwk'}));
}
export async function parsePrivateKey(data: Buffer, password?: string): Promise<ParsedKey> {
  const text=data.toString('utf8');
  if(text.includes('BEGIN ENCRYPTED PRIVATE KEY')&&password===undefined)throw new Error('encrypted key requires a passphrase');
  let parsed=utils.parseKey(/^PuTTY-User-Key-File-[23]:/.test(text)?await ppk3(text,password):data,password);
  if(parsed instanceof Error && /BEGIN (?:ENCRYPTED )?PRIVATE KEY/.test(text)){
    parsed=utils.parseKey(exportForSsh(createPrivateKey({key:data,format:'pem',passphrase:password})));
  }
  if(parsed instanceof Error)throw parsed;
  return Array.isArray(parsed)?parsed[0]:parsed;
}
