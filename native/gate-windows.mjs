// Explicit GUI probe, not run by native unit tests. All artifacts live in project tmp/.
import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { resolve, join } from 'node:path';
import { mkdir, writeFile, readFile } from 'node:fs/promises';
import assert from 'node:assert/strict';

if (!process.argv.includes('--run-gui') || process.platform !== 'win32') {
  console.error('Blocked: pass --run-gui on an interactive Windows desktop with permission to create six test terminals.');
  process.exit(2);
}
const root=resolve('tmp/native-gate'); await mkdir(root,{recursive:true});
const fixture=join(root,"same folder Ω ' & ; % [data]"); await mkdir(fixture,{recursive:true});
const helperDir=resolve('tmp/native/win-x64');const exe=join(helperDir,'Shellfox.Native.exe');
const ps=join(process.env.SystemRoot,'System32/WindowsPowerShell/v1.0/powershell.exe');
const pwsh=join(process.env.ProgramFiles,'PowerShell/7/pwsh.exe');
const gateShell=join(root,'shell');await mkdir(gateShell,{recursive:true});
for(const agent of ['alpha','beta'])await writeFile(join(root,`agent-${agent}.cjs`),'setInterval(() => {}, 1000);\n');
const literal=s=>"'"+s.replaceAll("'","''")+"'";
await writeFile(join(gateShell,'bootstrap.ps1'),`param([string]$TicketPath)
& ${literal(resolve('resources/shell/bootstrap.ps1'))} -TicketPath $TicketPath
$kind = if ($PSVersionTable.PSVersion.Major -ge 7) { 'alpha' } else { 'beta' }
& ${literal(process.execPath)} (Join-Path ${literal(root)} ("agent-" + $kind + ".cjs"))
`);
const events=[];const pending=new Map();let input='';
let broker;let dead=false;
function start(){
 broker=spawn(exe,['broker'],{shell:false,windowsHide:true,stdio:'pipe'});dead=false;input='';
 broker.stderr.resume();
 broker.on('error',e=>{for(const p of pending.values())p.reject(e);pending.clear();dead=true;});
 broker.on('exit',()=>{dead=true;for(const p of pending.values())p.reject(Error('broker exited'));pending.clear();});
 broker.stdout.on('data',chunk=>{input+=chunk.toString('utf8');let i;while((i=input.indexOf('\n'))>=0){const f=JSON.parse(input.slice(0,i));input=input.slice(i+1);if(f.kind==='event')events.push(f.event);else{const p=pending.get(f.id);if(p){pending.delete(f.id);p.resolve(f.result);}}}});
}
function request(method,payload){assert(!dead,'broker is alive');const id=randomUUID();return new Promise((resolve,reject)=>{const timer=setTimeout(()=>{pending.delete(id);reject(Error(`${method} timeout`));},10000);pending.set(id,{resolve:r=>{clearTimeout(timer);resolve(r);},reject:e=>{clearTimeout(timer);reject(e);}});broker.stdin.write(JSON.stringify({version:1,id,kind:'request',method,payload})+'\n');});}
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function until(predicate,timeout=18000){const end=Date.now()+timeout;while(Date.now()<end){const value=predicate();if(value)return value;await sleep(100);}throw Error('Observation deadline exceeded');}
async function powershell(code){return new Promise((resolve,reject)=>{const encoded=Buffer.from("$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';"+code+";exit 0",'utf16le').toString('base64');const p=spawn(ps,['-NoProfile','-EncodedCommand',encoded],{shell:false,windowsHide:true,stdio:'pipe'});let out='',err='';p.stdout.on('data',c=>out+=c);p.stderr.on('data',c=>err+=c);const timer=setTimeout(()=>{p.kill();reject(Error('PS probe timed out'));},15000);p.on('exit',c=>{clearTimeout(timer);c===0?resolve(out):reject(Error(`PS probe exit ${c}: ${err}`));});});}
const registrations=[];const targets=[];const results={ gate:'incomplete',commands:['node native/gate-windows.mjs --run-gui'],shells:[],focus:[],assertions:[],registry:'not modified' };
const init={userDataDir:join(root,'user-data'),helperDir,shellScriptDir:gateShell,packagedExecutable:null};
let watch;
async function agentRows(){
 const roots=Buffer.from(JSON.stringify(registrations.map(r=>r.shell))).toString('base64');
 const code=`$roots=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('${roots}')) | ConvertFrom-Json; $ids=@($roots | ForEach-Object { $p=Get-Process -Id $_.pid -ErrorAction SilentlyContinue; if($p -and $p.StartTime.ToFileTimeUtc().ToString() -eq $_.startTime) { $p.Id } }); @(Get-CimInstance Win32_Process | Where-Object { $ids -contains [int]$_.ParentProcessId -and $_.Name -eq 'node.exe' -and $_.CommandLine -and ($_.CommandLine.Contains(${literal(join(root,'agent-alpha.cjs'))}) -or $_.CommandLine.Contains(${literal(join(root,'agent-beta.cjs'))})) } | ForEach-Object { @{pid=[int]$_.ProcessId; parentPid=[int]$_.ParentProcessId; startTime=(Get-Process -Id $_.ProcessId).StartTime.ToFileTimeUtc().ToString(); executable=$_.ExecutablePath} }) | ConvertTo-Json -Compress`;
 const out=(await powershell(code)).trim();const rows=out?JSON.parse(out):[];return Array.isArray(rows)?rows:[rows];
}
try{
 start();const probe=await request('initialize',init);assert(probe.ok);assert(probe.value.available);results.probe=probe.value;
 for(let s=0;s<2;s++){
  const sessionId=randomUUID();let target=null;
  for(let t=0;t<3;t++){
   const tabId=randomUUID(),operationId=randomUUID();
   const shellId=t===2?'windows-powershell':'pwsh';
   const launch={sessionId,tabId,operationId,cwd:fixture,shellId,shellExecutable:shellId==='pwsh'?pwsh:ps,windowName:`shellfox-${sessionId}`,titleMarker:`SHELLFOX:${sessionId}:${tabId}`,existingTarget:target};
   const receipt=await request('launch',launch);assert(receipt.ok);
   const event=await until(()=>events.find(e=>e.type==='registered' && e.registration.operationId===operationId));
   registrations.push(event.registration);
   results.shells.push({registration:event.registration,target:event.target,receipt});
   assert(event.target,'Verified target was found');
   if(target)assert.equal(event.target.hwnd,target.hwnd);
   target=event.target;
  }
  targets.push(target);
 }
 assert.notEqual(targets[0].hwnd,targets[1].hwnd);assert.equal(new Set(registrations.map(r=>r.shell.pid)).size,6);
 results.assertions.push('Two distinct HWNDs, three independently authenticated shell registrations each, shared adversarial CWD. Both PowerShell versions retain profiles.');
 const rules=['alpha','beta'].map(label=>({id:randomUUID(),label,enabled:true,executableBasenames:['node.exe'],executablePaths:[],scriptPathSuffixes:[`agent-${label}.cjs`]}));
 // Duplicate matching rule must not double-count an agent identity.
 rules.push({...rules[0],id:randomUUID()});
 watch={tabs:registrations.map(registration=>({sessionId:registration.sessionId,tabId:registration.tabId,registration})),rules};
 assert((await request('setWatch',watch)).ok);
 results.runningObservations=(await until(()=>events.find(e=>e.type==='observations' && e.items.length===6 && e.items.every(i=>i.root==='alive' && i.health==='healthy' && i.agents===1)),25000)).items;
 results.assertions.push('Six roots independently report exactly one scoped agent, despite duplicate matching rules. Alpha and beta fixture scripts match separate script identities.');
 results.agentIdentities=await agentRows();assert.equal(results.agentIdentities.length,6);
 for(const target of targets){
  // Native focus can honestly return FOCUS_DENIED. Either way it must not create shells/tabs.
  const before=registrations.length;
  assert((await request('verifyTarget',target)).ok);
  await powershell(`Add-Type -TypeDefinition 'using System;using System.Runtime.InteropServices;public static class GateMin { [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int c); }';[GateMin]::ShowWindow([IntPtr][long]${target.hwnd},6) | Out-Null`);
  const focus=await request('focus',target);results.focus.push({hwnd:target.hwnd,minimizedFirst:true,result:focus});
  assert(focus.ok || focus.error.code==='FOCUS_DENIED');assert.equal(registrations.length,before);
 }
 // Manager shutdown must leave roots and fixture agents alive. Restart reuses exact identities, never launches.
 broker.stdin.end();await until(()=>dead);const stillAlive=await agentRows();assert.equal(stillAlive.length,6);
 results.assertions.push('Closing broker stdin left all six registered shells and their agents running.');
 const restartEventIndex=events.length;
 start();assert((await request('initialize',init)).ok);assert((await request('setWatch',watch)).ok);
 for(const target of targets)assert((await request('verifyTarget',target)).ok);
 results.reconnectedObservations=(await until(()=>events.slice(restartEventIndex).find(e=>e.type==='observations' && e.items.length===6 && e.items.every(i=>i.root==='alive' && i.health==='healthy' && i.agents===1)),25000)).items;
 results.assertions.push('Fresh broker accepted persisted exact shell identities and verified original HWNDs without launching terminals.');
 // Stop only one recorded test agent with PID/time identity recheck. Its shell must become waiting while others stay running.
 const first=results.agentIdentities[0];
 await powershell(`$p=Get-Process -Id ${first.pid} -ErrorAction Stop;if($p.StartTime.ToFileTimeUtc().ToString() -ne '${first.startTime}') { throw 'identity changed' };Stop-Process -Id $p.Id`);
 const stoppedRoot=registrations.find(r=>r.shell.pid===first.parentPid);
 results.afterSingleAgentExit=(await until(()=>events.filter(e=>e.type==='observations').findLast(e=>e.items.length===6 && e.items.every(i=>i.root==='alive' && i.health==='healthy' && i.agents===(i.tabId===stoppedRoot.tabId?0:1))),25000)).items;
 results.assertions.push('Stopping one recorded fixture agent changed only its own root to zero agents.');
 const stale={...targets[0],hwnd:'1'};const staleFocus=await request('focus',stale);assert(!staleFocus.ok && staleFocus.error.code==='TARGET_LOST');results.stale=staleFocus;
 const missing=await request('launch',{sessionId:randomUUID(),tabId:randomUUID(),operationId:randomUUID(),cwd:fixture,shellId:'pwsh',shellExecutable:'C:\\missing\\pwsh.exe',windowName:'invalid',titleMarker:'invalid',existingTarget:null});assert(!missing.ok);results.missingShell=missing;
 const explorer=await request('setExplorerIntegration',{installed:true,executablePath:process.execPath});assert(!explorer.ok && explorer.error.code==='UNSUPPORTED');results.assertions.push('Stale HWND failed closed, malformed/missing-shell launch rejected, unpackaged registry installation rejected.');
 assert((await request('verifyTarget',targets[1])).ok);
 await powershell(`Add-Type -TypeDefinition 'using System;using System.Runtime.InteropServices;public static class GateRename { [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern bool SetWindowText(IntPtr h,string s); }';if(![GateRename]::SetWindowText([IntPtr][long]${targets[1].hwnd},'Shellfox TEST renamed title')) { throw 'title change denied' }`);
 const renamed=await request('focus',targets[1]);assert(!renamed.ok && renamed.error.code==='TARGET_LOST');results.renamedTitle=renamed;
 results.assertions.push('Changing the recorded test HWND title made focus fail closed without dispatching wt.exe or launching shells. This is title verification evidence, not support for internal WT renaming.');
 results.gate='authenticated-placement-tracking-proven';
 results.remaining=['Interactive manual Explorer folder/background clicks, installation/update/uninstall and packaged tests are not covered.','No renamed/dragged-tab guarantee. Focus denial was not forced; any observed denial is recorded.','Minimized restore tested through the resident broker. No UI Automation, stable tab activation or command exit status.','Agents are controlled alpha/beta fixtures, not proof of every installed agent layout.'];
}catch(e){results.gate='blocked';results.error=e.stack;process.exitCode=1;}
finally{
 // Cleanup checks every shell/agent creation time and leaves anything unexpected alone.
 results.cleanup=[];
 // A failed placement assertion can happen before the normal agent snapshot.
 // Discover only exact test-script children of the recorded PID/time roots for cleanup.
 if(!results.agentIdentities){try{results.agentIdentities=await agentRows();}catch(e){results.cleanupDiscoveryError=String(e);}}
 for(const registration of registrations){
  try{
   const agents=(results.agentIdentities??[]).filter(p=>p.parentPid===registration.shell.pid);
   for(const p of agents)await powershell(`$p=Get-Process -Id ${p.pid} -ErrorAction SilentlyContinue;if($p -and $p.StartTime.ToFileTimeUtc().ToString() -eq '${p.startTime}') { Stop-Process -Id $p.Id }`);
   const outcome=await powershell(`$p=Get-Process -Id ${registration.shell.pid} -ErrorAction SilentlyContinue;if($p -and $p.StartTime.ToFileTimeUtc().ToString() -eq '${registration.shell.startTime}') { $children=@(Get-CimInstance Win32_Process -Filter 'ParentProcessId=${registration.shell.pid}');if($children.Count -eq 0) { Stop-Process -Id $p.Id; 'closed' } else { 'left with unexpected children' } } else { 'already absent or changed' }`);
   results.cleanup.push({shell:registration.shell,result:outcome.trim()});
  }catch(e){results.cleanup.push({shell:registration.shell,error:String(e)});}
 }
 if(results.cleanup.some(c=>c.error || c.result==='left with unexpected children')){results.gate='cleanup-validation-blocked';results.error=(results.error??'')+'\nAt least one recorded test shell could not be safely cleaned up. Inspect cleanup entries.';process.exitCode=1;}
 if(broker && !dead){
  if(results.gate==='authenticated-placement-tracking-proven' && results.cleanup.every(c=>c.result==='closed')){
   try{
    await until(()=>events.filter(e=>e.type==='observations').at(-1)?.items.length===6 && events.filter(e=>e.type==='observations').at(-1).items.every(i=>i.root==='exited'),10000);
    // WT can retain dead tabs after Stop-Process. Close only the six recorded dead fixture tabs,
    // after UIA confirms the exact three expected titles in each HWND. This is test cleanup only.
    await writeFile(join(root,'result.json'),JSON.stringify(results,null,2));
    await powershell(`& ${literal(resolve('native/close-gate-windows.ps1'))} -ReportPath ${literal(join(root,'result.json'))}`);
    results.closedTargets=[];
    for(const target of targets){const response=await request('focus',target);assert(!response.ok && response.error.code==='TARGET_LOST');results.closedTargets.push(response);}
    results.assertions.push('All six exact test shells closed and observations were exited. UIA confirmed three expected fixture tabs per HWND before test-only cleanup. Focusing the destroyed HWNDs then failed without replacement windows.');
   }catch(e){results.gate='cleanup-validation-blocked';results.error=String(e);process.exitCode=1;}
  }
  broker.stdin.end();await until(()=>dead,4000).catch(()=>broker.kill());
 }
 results.targetLostEvents=events.filter(e=>e.type==='target-lost');
 await writeFile(join(root,'result.json'),JSON.stringify(results,null,2));
 console.log(JSON.stringify({gate:results.gate,assertions:results.assertions.length,report:join(root,'result.json'),error:results.error}));
}
