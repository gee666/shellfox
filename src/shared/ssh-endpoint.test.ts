import { expect, it } from 'vitest';
import { normalizeSshEndpoint } from './ssh-endpoint';
import { sshProfileInputSchema } from './ssh-schemas';
const endpoint=(host:string,user='configured',port=22)=>normalizeSshEndpoint({host,user,port});
it('uses the LAST @ prefix as username, overriding UserName',()=>{
 expect(endpoint('login@server')).toEqual({host:'server',user:'login',port:22});
 expect(endpoint('user@realm@server')).toEqual({host:'server',user:'user@realm',port:22});
 expect(endpoint('@server')).toEqual({host:'server',user:'',port:22});
});
it('mirrors ASCII whitespace and ignored single-colon suffix without guessing the port',()=>{
 expect(endpoint(' \tlogin@ser ver\t.test:2222','old',2200)).toEqual({host:'server.test',user:'login',port:2200});
 expect(endpoint('host:not-a-port','old',2222)).toEqual({host:'host',user:'old',port:2222});
 expect(endpoint('host:one:two').host).toBe('host:one:two');
});
it('protects IPv6 colons and strips literal brackets like the PuTTY resolver',()=>{
 expect(endpoint('login@[2001:db8::1]:99')).toEqual({host:'2001:db8::1',user:'login',port:22});
 expect(endpoint('2001:db8::1').host).toBe('2001:db8::1');
 expect(endpoint('[fe80::1%eth0]').host).toBe('fe80::1%eth0');
 expect(endpoint('[not-ipv6]').host).toBe('[not-ipv6]');
});
it('normalizes BEFORE IPC/preload validation and rejects remaining whitespace/control characters',()=>{
 const p={id:'00000000-0000-4000-8000-000000000001',name:'test',host:' me@server ',user:'old',port:22,keyFile:null,remoteCwd:null};
 expect(sshProfileInputSchema.parse(p)).toMatchObject({host:'server',user:'me'});
 expect(sshProfileInputSchema.safeParse({...p,host:'server\u00a0name'}).success).toBe(false);
 expect(sshProfileInputSchema.safeParse({...p,host:'server\nname'}).success).toBe(false);
});
