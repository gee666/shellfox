import { beforeEach, afterEach, expect, it } from 'vitest';
import { mkdtemp, mkdir, rm, readFile, writeFile, stat, readdir } from 'node:fs/promises';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { SshProfileStore } from './storage';
import { parsePutty, parsePuttyFile, readPuttySessions } from './putty';
let directory: string, store: SshProfileStore;
const input = (name='prod') => ({ id:randomUUID(),name,host:'example.test',port:22,user:'deploy',keyFile:null,remoteCwd:'/srv' });
beforeEach(async()=>{await mkdir('tmp',{recursive:true});directory=await mkdtemp(path.resolve('tmp/ssh-storage-'));store=new SshProfileStore(directory);});
afterEach(async()=>{await rm(directory,{recursive:true,force:true});});
it('persists atomically, sorts names, keeps/clears/replaces secrets and never exposes passwords',async()=>{
  expect(await store.list()).toEqual({ok:true,value:[]});
  const p=input('Zulu');await store.save({...p,password:'secret'});await store.save(input('alpha'));
  expect(await store.save(p)).toMatchObject({ok:true,value:[{name:'alpha'},{name:'Zulu',hasPassword:true}]});
  expect(JSON.stringify(await store.list())).not.toContain('secret');
  expect((await store.read()).find(v=>v.id===p.id)?.password).toBe('secret');
  await store.save({...p,password:null});expect((await store.read()).find(v=>v.id===p.id)?.password).toBe(null);
  await store.save({...p,password:'replacement'});expect((await store.read()).find(v=>v.id===p.id)?.password).toBe('replacement');
  expect((await readdir(directory)).filter(n=>n.endsWith('.tmp'))).toEqual([]);
  if(process.platform!=='win32')expect((await stat(store.file)).mode&0o777).toBe(0o600);
  await store.delete(p.id);expect(await store.list()).toMatchObject({ok:true,value:[{name:'alpha'}]});
});
it('serializes concurrent saves and rejects case-insensitive names or invalid ports',async()=>{
  await Promise.all([store.save(input('a')),store.save(input('b'))]);expect((await store.read()).length).toBe(2);
  expect(await store.save(input('A'))).toMatchObject({ok:false,error:{code:'VALIDATION',message:'A connection with this name already exists.'}});
  for(const port of [0,65536,1.5])expect(await store.save({...input(),port})).toMatchObject({ok:false,error:{code:'VALIDATION'}});
});
it('refuses malformed storage without overwriting it',async()=>{
  for(const text of ['broken',JSON.stringify({version:2,profiles:[]}),JSON.stringify({version:1,profiles:[{...input(),password:null,source:'manual'},{...input(),name:'PROD',password:null,source:'manual'}]})]){
    await writeFile(store.file,text);expect(await store.list()).toMatchObject({ok:false,error:{code:'STORAGE_FAILED'}});
    expect(await store.save(input())).toMatchObject({ok:false,error:{code:'STORAGE_FAILED'}});expect(await readFile(store.file,'utf8')).toBe(text);
  }
});
it('imports by name, preserving id, passwords, folder and manual source',async()=>{
  const p=input('Prod');await store.save({...p,password:'saved'});
  const sessions=[{name:'prod',host:'new.test',port:2222,user:'root',keyFile:'/key'},{name:'nas',host:'nas',port:22,user:'admin',keyFile:null}];
  expect(await store.import(sessions)).toMatchObject({ok:true,value:{added:1,updated:1,found:2}});
  const updated=(await store.read()).find(v=>v.id===p.id)!;expect(updated).toMatchObject({host:'new.test',password:'saved',remoteCwd:'/srv',source:'manual'});
  expect(await store.import(sessions)).toMatchObject({ok:true,value:{added:0,updated:0,found:2}});
  expect(await store.import([])).toMatchObject({ok:true,value:{found:0}});
});
it('normalizes manual saves and fixes legacy imports without losing credentials/folder/id',async()=>{
 const p=input('legacy');const raw={...p,host:'first@account@server.test:99',user:'wrong',password:'saved',source:'putty'};
 await writeFile(store.file,JSON.stringify({version:1,profiles:[raw]}));
 expect((await store.read())[0]).toMatchObject({host:'server.test',user:'first@account',port:22});
 expect(JSON.parse(await readFile(store.file,'utf8')).profiles[0].host).toBe(raw.host); // read is not a write
 expect(await store.import([{name:'legacy',host:raw.host,user:'wrong',port:22,keyFile:null}])).toMatchObject({ok:true,value:{found:1,added:0,updated:1}});
 expect(JSON.parse(await readFile(store.file,'utf8')).profiles[0]).toMatchObject({id:p.id,host:'server.test',user:'first@account',password:'saved',remoteCwd:'/srv'});
 expect(await store.save({...p,host:' me@other.test ',user:'ignored'})).toMatchObject({ok:true,value:[{host:'other.test',user:'me'}]});
});
it('preserves internal connection options but never exposes them as renderer DTO fields',async()=>{
 const p=input('proxy');await store.import([{...p,connectionOptions:{unsupportedProxy:{method:'SOCKS5',host:'proxy.test',port:1080},tryAgent:false}}]);
 expect((await store.read())[0].connectionOptions?.tryAgent).toBe(false);expect(JSON.stringify(await store.list())).not.toContain('connectionOptions');
 expect(await store.save({...p,id:(await store.read())[0].id,host:'direct.test'})).toMatchObject({ok:true});expect((await store.read())[0].connectionOptions?.unsupportedProxy?.method).toBe('SOCKS5');
});
it('parses PuTTY registry and Unix files with unicode and user@host',async()=>{
  expect(parsePutty('%E6%B5%8B%E8%AF%95%20prod',{HostName:'deploy@host',PortNumber:2222,PublicKeyFile:'C:\\keys\\key.ppk'})).toEqual({name:'测试 prod',host:'host',user:'deploy',port:2222,keyFile:'C:\\keys\\key.ppk'});
  expect(parsePutty('Default%20Settings',{})).toBe(null);
  expect(parsePutty('telnet',{Protocol:'telnet',HostName:'host'})).toBe(null);
  expect(parsePuttyFile('test%20name','HostName=user@host\nPortNumber=22\nProtocol=ssh\nUserName=explicit\nPublicKeyFile=/key=path\n')).toMatchObject({name:'test name',host:'host',user:'user',keyFile:'/key=path'});
  expect(await readPuttySessions({platform:'linux',home:directory})).toEqual([]);
  const sessions=path.join(directory,'.putty/sessions');await mkdir(sessions,{recursive:true});await writeFile(path.join(sessions,'prod%20web'),'HostName=host\nProtocol=ssh\n');
  expect(await readPuttySessions({platform:'linux',home:directory})).toMatchObject([{name:'prod web'}]);
  expect(await readPuttySessions({platform:'win32',run:async()=>({ok:true,value:[]})})).toEqual([]);
  expect(await readPuttySessions({platform:'win32',run:async()=>({ok:true,value:[{name:'%E6%B5%8B%E8%AF%95',values:{HostName:'host'}}]})})).toMatchObject([{name:'测试'}]);
});
