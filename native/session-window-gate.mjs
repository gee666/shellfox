import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { randomUUID } from 'node:crypto';
import { resolve, join } from 'node:path';
import { mkdir, writeFile, readFile } from 'node:fs/promises';
import assert from 'node:assert/strict';
const exec=promisify(execFile);
if(process.platform!=='win32'||!process.argv.includes('--run-gui')){console.error('Explicit --run-gui Windows desktop permission required.');process.exit(2);}
const root=resolve('tmp/session-window-gate',randomUUID());await mkdir(root,{recursive:true});
const shellDir=join(root,'shell');await mkdir(shellDir);
const ps=join(process.env.SystemRoot,'System32/WindowsPowerShell/v1.0/powershell.exe');
const pwsh=join(process.env.ProgramFiles,'PowerShell/7/pwsh.exe');
const literal=s=>"'"+s.replaceAll("'","''")+"'";
// Test-only mailbox invokes one shipped integration script in the real shell, not a wrapper.
await writeFile(join(shellDir,'bootstrap.ps1'),`param([string]$TicketPath)
$t=[IO.File]::ReadAllText($TicketPath)|ConvertFrom-Json
$box=Join-Path ${literal(root)} ($t.tabId+'.request.json');$ack=$box+'.done';$seen=''
& ${literal(resolve('resources/shell/bootstrap.ps1'))} -TicketPath $TicketPath
while($true){
 if([IO.File]::Exists($box)){$d=[IO.File]::ReadAllText($box)|ConvertFrom-Json;if($d.id -ne $seen){$seen=$d.id;& ${literal(resolve('resources/shell/register-session.ps1'))} -TicketPath $d.ticketPath;[IO.File]::WriteAllText($ack,$seen)}}
 Start-Sleep -Milliseconds 100
}
`);
const init={userDataDir:join(root,'data'),helperDir:resolve('tmp/native/win-x64'),shellScriptDir:shellDir,packagedExecutable:null};
// prepareRegistration resolves its shipped script from this isolated script directory.
await writeFile(join(shellDir,'register-session.ps1'),await readFile(resolve('resources/shell/register-session.ps1')));
const events=[],pending=new Map(),entries=[];let buffer='';
const broker=spawn(join(init.helperDir,'Shellfox.Native.exe'),['broker'],{shell:false,windowsHide:true,stdio:'pipe'});broker.stderr.resume();
broker.stdout.on('data',c=>{buffer+=c;let i;while((i=buffer.indexOf('\n'))>=0){const f=JSON.parse(buffer.slice(0,i));buffer=buffer.slice(i+1);if(f.kind.endsWith('event'))events.push(f.event);else{const p=pending.get(f.id);if(p){pending.delete(f.id);p(f.result);}}}});
const pause=ms=>new Promise(r=>setTimeout(r,ms));
async function until(f,ms=20000){let end=Date.now()+ms;while(Date.now()<end){const v=await f();if(v)return v;await pause(100);}throw Error('Timed out waiting for native evidence');}
function request(method,payload){const id=randomUUID();return new Promise((res,rej)=>{const t=setTimeout(()=>{pending.delete(id);rej(Error(method+' timeout'));},10000);pending.set(id,r=>{clearTimeout(t);res(r);});broker.stdin.write(JSON.stringify({version:1,kind:'request',id,method,payload})+'\n');});}
function launch(sessionId=randomUUID()){const tabId=randomUUID();return {sessionId,tabId,operationId:randomUUID(),cwd:root,shellId:'pwsh',shellExecutable:pwsh,windowName:'shellfox-'+sessionId,titleMarker:'SHELLFOX:'+sessionId+':'+tabId,existingTarget:null};}
async function adopted(entry,binding){const r=launch(binding.target.sessionId);const prep=await request('prepareRegistration',{request:r,binding});assert(prep.ok);const id=randomUUID();const box=join(root,entry.originalTabId+'.request.json');await writeFile(box,JSON.stringify({id,ticketPath:prep.value.ticketPath}));await until(async()=>{try{return (await readFile(box+'.done','utf8'))===id;}catch{return false;}});return {r,prep};}
async function clean(list,name){const file=join(root,name+'.json');await writeFile(file,JSON.stringify({shells:list,cleanup:list.map(e=>({shell:e.registration.shell}))},null,2));await exec(ps,['-NoProfile','-NonInteractive','-File',resolve('tests/windows/cleanup-gate.ps1'),'-ReportPath',file],{timeout:30000,windowsHide:true});}
const report={checks:[],entries,registry:'not touched',profiles:'not modified'};
try{
 assert((await request('initialize',init)).ok);
 for(let s=0;s<2;s++){const r=launch();assert((await request('launch',r)).ok);const reg=await until(()=>events.find(e=>e.type==='registered'&&e.registration.operationId===r.operationId));const bound=await until(()=>events.find(e=>e.type==='bound'&&e.binding.generation===r.operationId));entries.push({registration:reg.registration,target:reg.target,binding:bound.binding,originalTabId:r.tabId});}
 const a=entries[0],b=entries[1];
 const ticket=await adopted(a,a.binding);const same=await until(()=>events.find(e=>e.type==='adopted'&&e.source==='manual'&&e.member.registration.operationId===ticket.r.operationId));
 assert.equal(same.member.registration.shell.pid,a.registration.shell.pid);assert.equal(same.member.registration.tabId,a.registration.tabId);assert(same.member.wtSession);report.wtSession=same.member.wtSession;
 report.checks.push('Existing real PowerShell born before the ticket explicitly registered with WT_SESSION; no duplicate member or wrapper.');
 const wrong=await adopted(a,b.binding);assert(!events.some(e=>e.type==='adopted'&&e.member.registration.operationId===wrong.r.operationId));const bm=await request('getWindowMembership',b.binding);assert(bm.ok&&bm.value.members.length===1);
 report.checks.push('A registration command in the wrong physical window did not adopt or transfer its shell to the destination.');
 await adopted(a,a.binding); // Restore A's visible marker after the rejected challenge.
 const focused=await request('focusSessionWindow',a.binding);assert(focused.ok || focused.error.code==='FOCUS_DENIED');report.focus=focused;
 const premature=await request('reopenWindow',{request:launch(a.registration.sessionId),previous:a.binding});assert(!premature.ok&&premature.error.code==='RETRY_CONFIRM_REQUIRED');
 await clean([a],'closed-original');await until(()=>events.some(e=>e.type==='lost'&&e.binding.generation===a.binding.generation));
 const replacement=launch(a.registration.sessionId);assert((await request('reopenWindow',{request:replacement,previous:a.binding})).ok);
 const next=await until(()=>events.find(e=>e.type==='registered'&&e.registration.operationId===replacement.operationId));const nextBound=await until(()=>events.find(e=>e.type==='bound'&&e.binding.generation===replacement.operationId));entries.push({registration:next.registration,target:next.target,binding:nextBound.binding,originalTabId:replacement.tabId});
 assert.equal(next.registration.sessionId,a.registration.sessionId);assert.notEqual(next.registration.shell.pid,a.registration.shell.pid);assert.notEqual(nextBound.binding.generation,a.binding.generation);
 const stale=await request('focusSessionWindow',a.binding);assert(!stale.ok&&stale.error.code==='TARGET_LOST');
 const current=await request('getWindowMembership',nextBound.binding);assert(current.ok&&current.value.windowState==='alive');
 report.checks.push('Confirmed closed HWND reopened into the same session with a new shell/generation; stale binding refused and no commands restored.');
 report.verdict='native-reopen-explicit-registration-proven';
}catch(e){report.verdict='blocked';report.error=String(e.stack??e);process.exitCode=1;}
finally{
 broker.stdin.end();await pause(300);
 try{await clean(entries,'cleanup');report.cleanup='only recorded test roots and UUID tabs';}catch(e){report.cleanupError=String(e);process.exitCode=1;}
 await writeFile(join(root,'result.json'),JSON.stringify(report,null,2));console.log(JSON.stringify({verdict:report.verdict,checks:report.checks.length,report:join(root,'result.json'),error:report.error,cleanupError:report.cleanupError}));
}
