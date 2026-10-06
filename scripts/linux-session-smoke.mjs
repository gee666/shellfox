// LEGACY external GNOME/.NET backend only, not embedded-terminal acceptance.
// Real Linux/desktop opt-in. No installer/config mutation and no terminal-server kill.
import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import assert from 'node:assert/strict';
if(process.platform!=='linux'||process.env.SHELLFOX_LINUX_GUI!=='1')throw Error('Requires Linux desktop and SHELLFOX_LINUX_GUI=1');
const dir=path.resolve('tmp/linux-session-smoke',randomUUID());await mkdir(dir,{recursive:true});
const helperDir=path.resolve('tmp/native/linux-x64');const child=spawn(path.join(helperDir,'Shellfox.Linux'),['broker'],{stdio:'pipe',shell:false});child.stderr.resume();
const events=[],requests=new Map();let buffer='';
child.stdout.on('data',chunk=>{buffer+=chunk;let i;while((i=buffer.indexOf('\n'))>=0){const frame=JSON.parse(buffer.slice(0,i));buffer=buffer.slice(i+1);if(frame.kind==='event')events.push(frame.event);else{const callback=requests.get(frame.id);if(callback){requests.delete(frame.id);callback(frame.result);}}}});
function request(method,payload){const id=randomUUID();return new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error(method+' timeout')),10000);requests.set(id,r=>{clearTimeout(timer);resolve(r);});child.stdin.write(JSON.stringify({version:1,kind:'request',id,method,payload})+'\n');});}
const pause=ms=>new Promise(r=>setTimeout(r,ms));async function until(f){const end=Date.now()+25000;while(Date.now()<end){const v=f();if(v)return v;await pause(100);}throw Error('Native observation timeout');}
const roots=[];const report={roots,registry:'not touched'};
try{
 const probe=await request('initialize',{userDataDir:path.join(dir,'data'),helperDir,shellScriptDir:path.resolve('resources/shell-linux'),packagedExecutable:null});assert(probe.ok&&probe.value.available,JSON.stringify(probe));report.probe=probe;
 for(let s=0;s<2;s++){const sessionId=randomUUID(),tabId=randomUUID(),operationId=randomUUID();const launch={sessionId,tabId,operationId,cwd:dir,shellId:'bash',shellExecutable:probe.value.shells[0].executable,windowName:'shellfox-'+sessionId,titleMarker:'SHELLFOX:'+sessionId+':'+tabId,existingTarget:null};assert((await request('launch',launch)).ok);const event=await until(()=>events.find(e=>e.type==='registered'&&e.registration.operationId===operationId));roots.push(event.registration);}
 assert.notEqual(roots[0].shell.pid,roots[1].shell.pid);assert((await request('setWatch',{tabs:roots.map(registration=>({sessionId:registration.sessionId,tabId:registration.tabId,registration})),rules:[]})).ok);
 report.observations=(await until(()=>events.find(e=>e.type==='observations'&&e.items.length===2&&e.items.every(x=>x.root==='alive'&&x.health==='healthy')))).items;
 assert.equal((await request('focus',{})).error.code,'UNSUPPORTED');report.result='two authenticated Bash roots and healthy scoped polling; no focus/reopen claim';
}catch(e){report.error=String(e.stack??e);process.exitCode=1;}
finally{
 child.stdin.end();await pause(300);report.cleanup=[];
 for(const r of roots){try{const stat=await readFile(`/proc/${r.shell.pid}/stat`,'utf8');const fields=stat.slice(stat.lastIndexOf(')')+2).trim().split(/\s+/);const boot=(await readFile('/proc/sys/kernel/random/boot_id','utf8')).trim().replaceAll('-','');const identity=(BigInt('0x'+boot)*10n**20n+BigInt(fields[19])).toString();if(identity!==r.shell.startTime){report.cleanup.push('PID changed; untouched');continue;}const procdirs=await import('node:fs/promises');let children=0;for(const d of await procdirs.readdir('/proc')){if(!/^\d+$/.test(d))continue;try{const st=await readFile('/proc/'+d+'/stat','utf8');if(Number(st.slice(st.lastIndexOf(')')+2).trim().split(/\s+/)[1])===r.shell.pid)children++;}catch{}}if(children)throw Error('Unexpected children; root left untouched');process.kill(r.shell.pid,'SIGTERM');report.cleanup.push('terminated exact own Bash root');}catch(e){report.cleanup.push(String(e));process.exitCode=1;}}
 await writeFile(path.join(dir,'result.json'),JSON.stringify(report,null,2));console.log(JSON.stringify({report:path.join(dir,'result.json'),result:report.result,error:report.error}));
}
