import assert from 'node:assert/strict';
import path from 'node:path';
import { mkdir, writeFile, readFile } from 'node:fs/promises';
import { generateKeyPairSync } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { spawn } from 'node-pty';
import { sshFixture } from './ssh-server';
import { SshProfileStore } from '../../src/main/ssh/storage';
import { shellfoxShims } from '../../src/main/platform/shellfox-cli';
import { linuxLauncher } from '../../src/main/platform/linux-cli';
import { remoteCommand } from '../../src/main/ssh/presentation';
import { generatedTestKey, generatedPpk, TEST_KEY_PASSPHRASE } from './ssh-test-keys';
async function main(){
 const root=process.cwd(),directory=process.argv[2],helper=process.argv[3],windows=process.platform==='win32',fixture=await sshFixture();
 const folder=path.join(directory,'shims');await mkdir(folder,{recursive:true});
 const options={executable:process.execPath,cliHelper:helper,userData:directory};
 const shim=path.join(folder,windows?'shellfox.cmd':'shellfox'),shims=windows?shellfoxShims(options):null;
 await writeFile(shim,shims?.cmd??linuxLauncher(options),{mode:0o755});if(shims)await writeFile(path.join(folder,'shellfox'),shims.posix,{mode:0o755});
 const store=new SshProfileStore(directory),profile={id:'00000000-0000-4000-8000-000000000001',name:'fixture',host:'127.0.0.1',port:fixture.port,user:'fixture',password:'fixture-secret',keyFile:null,remoteCwd:null};
 await store.save(profile);const evidence:unknown[]=[];
 type Mode='password'|'key'|'cancel'|'changed'|'lost'|'refused'|'prompt'|'auth-failed';
 async function interactive(shell:string,args:string[],mode:Mode='password'){
  const before=fixture.records.bytes.length,authBefore=fixture.records.auth.length,windowsBefore=fixture.records.windows.length,commandsBefore=fixture.records.commands.length,shellsBefore=fixture.records.shells;
  const p=spawn(shell,args,{cwd:root,cols:90,rows:25,env:{...process.env,NO_COLOR:'1'}});
  let output='',trust=false,ready=false,sent=false,login=false,passphrase=false,passwords=0;
  const result=await new Promise<number>((resolve,reject)=>{
   const timer=setTimeout(()=>{p.kill();reject(new Error('interactive timeout: '+shell+'\n'+output));},15000);
   p.onData(text=>{
    output+=text;
    if(mode==='cancel'&&output.includes('select  enter connect')){p.write('\x1b');return;}
    if(mode==='changed'||mode==='refused')return;
    if(!login&&output.includes('login as:')){login=true;p.write('fixture\r');}
    if(!passphrase&&output.includes('Passphrase:')){passphrase=true;setTimeout(()=>p.write('fixture-passphrase\r'),500);}
    const prompts=(output.match(/fixture@127\.0\.0\.1's password:/g)||[]).length;
    if(prompts>passwords){passwords=prompts;p.write((passwords===1?'wrong':'fixture-secret')+'\r');}
    if(!trust&&output.includes('Trust this host? [y/N]')){trust=true;p.write('y\r');}
    if(!ready&&output.includes('REMOTE READY')){ready=true;if(mode==='lost'){p.write('DROP');return;}p.resize(110,31);setTimeout(()=>p.write('\x1b[A\t\x1b\x03é'),200);}
    if(!sent&&output.includes('REMOTE BYTES')){sent=true;setTimeout(()=>p.write('\x04'),250);}
   });
   p.onExit(e=>{clearTimeout(timer);setTimeout(()=>resolve(e.exitCode),300);});
  });
  evidence.push({shell,mode,result,output});assert.doesNotMatch(output,/reconnecting|reconnected/);
  if(mode==='cancel'){assert.equal(result,130);return;}
  if(mode==='changed'){assert.equal(result,1);assert.match(output,/host key changed/);assert.equal(fixture.records.auth.length,authBefore);return;}
  if(mode==='auth-failed'){assert.equal(result,1,output);assert.match(output,/authentication failed/);return;}
  if(mode==='lost'){assert.equal(result,255,output);assert.equal((output.match(/  connection lost/g)||[]).length,1);assert.equal(fixture.records.shells-shellsBefore,1);return;}
  if(mode==='refused'){assert.equal(result,255,output);assert.match(output,/ECONNREFUSED/);assert.equal((output.match(/connecting to/g)||[]).length,1);return;}
  assert.equal(result,7,output);assert.match(output,/connection to fixture closed.*exit 7/);
  if(fixture.records.commands.length===commandsBefore)assert.equal(fixture.records.shells-shellsBefore,1);
  else assert.ok(fixture.records.commands.slice(commandsBefore).every(c=>c.startsWith('cd -- ')));
  const bytes=Buffer.concat(fixture.records.bytes.slice(before));assert.ok(bytes.includes(Buffer.from('\x1b[A\t\x1b\x03é')));assert.ok(bytes.includes(4));
  assert.ok(fixture.records.windows.length>windowsBefore);assert.ok(!output.includes('fixture-secret')&&!output.includes('fixture-passphrase'));
  assert.ok(fixture.records.auth.slice(authBefore).includes(mode==='key'?'publickey':'password'));
  assert.equal((fixture.records.pty.at(-1) as {term:string}).term,'xterm-256color');
  // ssh2's fixture decoder starts encoded tty modes at offset 1; verify
  // RFC mode bytes independently in pty.test.ts and against real sshd.
 }
 try{
  const cmdArgs=['/d','/c','call',shim,'ssh','--profile','fixture'],terminal=windows?'cmd.exe':'/bin/sh',plainArgs=windows?cmdArgs:[shim,'ssh','fixture'];
  const encryptedKey=path.join(directory,'generated-ed25519-v3.ppk');
  await writeFile(encryptedKey,await generatedPpk(generatedTestKey('ed25519'),3,{passphrase:TEST_KEY_PASSPHRASE,newline:'\r\n'}),{mode:0o600});
  await store.save({...profile,password:null,keyFile:encryptedKey});await interactive(terminal,plainArgs,'key');await store.save(profile);
  await interactive(terminal,plainArgs);await interactive(terminal,plainArgs,'lost');
  fixture.settings.denyAuth=true;await interactive(terminal,plainArgs,'auth-failed');fixture.settings.denyAuth=false;
  await new Promise<void>(resolve=>fixture.server.close(()=>resolve()));await interactive(terminal,plainArgs,'refused');await new Promise<void>(resolve=>{fixture.server.listen(fixture.port,'0.0.0.0',resolve);});
  if(windows){
   await interactive('powershell.exe',['-NoLogo','-NoProfile','-Command',`& '${shim}' ssh -p fixture; exit $LASTEXITCODE`]);
   await interactive(path.join(process.env.ProgramFiles||'C:\\Program Files','PowerShell/7/pwsh.exe'),['-NoLogo','-NoProfile','-Command',`& '${shim}' ssh fixture; exit $LASTEXITCODE`]);
   await interactive('C:\\Program Files\\Git\\bin\\bash.exe',['-c',`'${path.join(folder,'shellfox').replaceAll('\\','/')}' ssh fixture`]);
   await interactive('cmd.exe',['/d','/c','call',shim,'ssh'],'cancel');
   if(process.env.SHELLFOX_SSH_TEST_WSL==='1'){
    const translate=(p:string)=>execFileSync('wsl.exe',['-d','Debian','--exec','wslpath','-u',p],{encoding:'utf8',timeout:10000}).trim();
    await interactive('wsl.exe',['-d','Debian','--exec','/bin/sh',translate(path.join(folder,'shellfox')),'ssh','fixture']);
    if(process.env.SHELLFOX_SSH_TEST_LINUX_ELECTRON){
     const native=path.join(folder,'linux-shellfox');await writeFile(native,linuxLauncher({executable:translate(process.env.SHELLFOX_SSH_TEST_LINUX_ELECTRON),cliHelper:translate(helper),userData:translate(directory)}));
     await interactive('wsl.exe',['-d','Debian','--exec','/bin/sh',translate(native),'ssh','fixture']);
    }
   }
  }
  await store.save({...profile,user:'',password:null});await interactive(terminal,plainArgs,'prompt');await store.save(profile);
  const keyFile=path.join(directory,'key.pem');await writeFile(keyFile,generateKeyPairSync('ed25519').privateKey.export({type:'pkcs8',format:'pem'}).toString(),{mode:0o600});
  const cwd="~/it's $(echo unsafe)";await store.save({...profile,keyFile,remoteCwd:cwd,password:null});await interactive(terminal,plainArgs,'key');assert.equal(fixture.records.commands.at(-1),remoteCommand(cwd));
  const hostsFile=path.join(directory,'ssh-known-hosts.json'),hosts=JSON.parse(await readFile(hostsFile,'utf8'));for(const host of Object.values(hosts.hosts) as {fingerprint:string}[])host.fingerprint='SHA256:changed';await writeFile(hostsFile,JSON.stringify(hosts));await interactive(terminal,plainArgs,'changed');
  console.log(JSON.stringify({ok:true,evidence,records:{pty:fixture.records.pty,windows:fixture.records.windows,auth:fixture.records.auth,commands:fixture.records.commands}},null,2));
 }finally{await fixture.close();}
 process.exit(0);
}
void main().catch(e=>{console.error(e);process.exit(1);});
