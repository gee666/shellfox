import { expect, it, vi } from 'vitest';
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import path from 'node:path';
import { PassThrough } from 'node:stream';
import { Socket } from 'node:net';
import { utils } from 'ssh2';
import { sshFixture } from '../../../tests/fixtures/ssh-server';
import { connectSsh, SshConnectionError } from './client';
import { trustHost } from './known-hosts';
import { CliTerminal } from './terminal';
const delay=(ms:number)=>new Promise(resolve=>setTimeout(resolve,ms));
it('refuses unsupported PuTTY proxies before DNS/auth prompts',async()=>{
 const terminal=new CliTerminal();const prompt=vi.spyOn(terminal,'prompt');
 await expect(connectSsh({id:'00000000-0000-4000-8000-000000000001',name:'proxy',host:'never-contact.invalid',port:22,user:'',password:null,keyFile:null,remoteCwd:null,source:'putty',connectionOptions:{unsupportedProxy:{method:'SSH jump host',host:'jump.invalid',port:22}}},'tmp/ssh-proxy-unused',terminal)).rejects.toBeInstanceOf(SshConnectionError);
 expect(prompt).not.toHaveBeenCalled();prompt.mockRestore();
});
it('keeps an idle session alive across many keepalives and delayed replies, with TCP keepalive and no idle timeout',async()=>{
 await mkdir('tmp',{recursive:true});const directory=await mkdtemp(path.resolve('tmp/ssh-idle-')),fixture=await sshFixture();
 const parsed=utils.parseKey(fixture.hostKey);if(parsed instanceof Error||Array.isArray(parsed))throw new Error('fixture host key');
 await trustHost(directory,'127.0.0.1',fixture.port,parsed.getPublicSSH(),async()=>true);
 const input=new PassThrough(),output=Object.assign(new PassThrough(),{columns:80,rows:24}),bytes:Buffer[]=[];output.on('data',chunk=>bytes.push(chunk));
 fixture.settings.colors=true;fixture.settings.rejectEnv=true;vi.stubEnv('NO_COLOR','1');
 const terminal=new CliTerminal(),keepalive=vi.spyOn(Socket.prototype,'setKeepAlive');let socket:Socket|undefined,ended=false;
 let announce!:()=>void;const ready=new Promise<void>(resolve=>{announce=resolve;});
 const session=connectSsh({id:'00000000-0000-4000-8000-000000000001',name:'idle',host:'127.0.0.1',port:fixture.port,user:'fixture',password:'fixture-secret',keyFile:null,remoteCwd:null,source:'manual'},directory,terminal,{keepaliveIntervalMs:20,stdin:input as unknown as NodeJS.ReadStream,stdout:output as unknown as NodeJS.WriteStream,onReady:announce,onSocket:s=>{socket=s;}}).then(code=>{ended=true;return code;},error=>{ended=true;throw error;});
 void session.catch(()=>{});
 try{
  await ready;expect(fixture.records.shells).toBe(1);expect(fixture.records.commands).toEqual([]);
  expect(fixture.records.env).toContainEqual({key:'COLORTERM',val:'truecolor'});
  expect(fixture.records.pty[0]).toMatchObject({term:'xterm-256color',cols:80,rows:24,width:0,height:0});
  expect(keepalive).toHaveBeenCalledWith(true,30000);expect(socket?.timeout).toBe(0);
  await delay(240);expect(fixture.records.keepalives).toBeGreaterThan(4);expect(ended).toBe(false);
  fixture.settings.stallKeepalives=true;const before=fixture.records.keepalives;
  await delay(400); // More than twenty intervals, far beyond ssh2's default 3.
  expect(fixture.records.keepalives-before).toBeGreaterThan(10);expect(ended).toBe(false);expect(socket?.destroyed).toBe(false);
  fixture.settings.stallKeepalives=false;fixture.flushKeepalives();await delay(160);expect(ended).toBe(false);
  input.write(Buffer.from([4]));expect(await session).toBe(7);
  expect(Buffer.concat(bytes).toString()).toContain('\x1b[31mRED\x1b[0m\x1b[38;5;202mINDEXED\x1b[0m\x1b[38;2;12;34;56mRGB\x1b[0m');
 }finally{terminal.abort.abort();await session.catch(()=>{});keepalive.mockRestore();vi.unstubAllEnvs();await fixture.close();await rm(directory,{recursive:true,force:true});}
},10000);
