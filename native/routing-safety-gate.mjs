// Focused fail-safe regression. Does not claim multi-tab acceptance.
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { randomUUID } from 'node:crypto';
import { resolve, join } from 'node:path';
import { mkdir, writeFile, readFile, readdir } from 'node:fs/promises';
import assert from 'node:assert/strict';
const exec=promisify(execFile);
if(process.platform!=='win32' || !process.argv.includes('--run-gui')){console.error('Requires --run-gui on an authorized Windows desktop.');process.exit(2);}
const root=resolve('tmp/routing-safety',randomUUID());await mkdir(root,{recursive:true});
const cwd=join(root,"same Ω ' & ; % [folder]");await mkdir(cwd,{recursive:true});
const ps=join(process.env.SystemRoot,'System32/WindowsPowerShell/v1.0/powershell.exe');
const pwsh=join(process.env.ProgramFiles,'PowerShell/7/pwsh.exe');
const helperDir=resolve('tmp/native/win-x64');
const init={userDataDir:join(root,'data'),helperDir,shellScriptDir:resolve('resources/shell'),packagedExecutable:null};
const literal=s=>"'"+s.replaceAll("'","''")+"'";
const pause=ms=>new Promise(r=>setTimeout(r,ms));
async function until(fn,ms=20000){const end=Date.now()+ms;while(Date.now()<end){const v=fn();if(v)return v;await pause(100);}throw Error('Native observation deadline');}
const events=[],pending=new Map();let child,input='',dead=true;
function start(){input='';dead=false;child=spawn(join(helperDir,'Shellfox.Native.exe'),['broker'],{shell:false,windowsHide:true,stdio:'pipe'});child.stderr.resume();child.on('error',fail);child.on('exit',()=>fail(Error('Broker exited')));child.stdout.on('data',c=>{input+=c.toString('utf8');let i;while((i=input.indexOf('\n'))>=0){const f=JSON.parse(input.slice(0,i));input=input.slice(i+1);if(f.kind==='event')events.push(f.event);else{const p=pending.get(f.id);if(p){pending.delete(f.id);p.resolve(f.result);}}}});}
function fail(e){dead=true;for(const p of pending.values())p.reject(e);pending.clear();}
function request(method,payload){const id=randomUUID();return new Promise((resolve,reject)=>{const timer=setTimeout(()=>{pending.delete(id);reject(Error(`${method} timed out`));},10000);pending.set(id,{resolve:r=>{clearTimeout(timer);resolve(r);},reject:e=>{clearTimeout(timer);reject(e);}});child.stdin.write(JSON.stringify({version:1,id,kind:'request',method,payload})+'\n');});}
async function stop(){if(!dead){child.stdin.end();await until(()=>dead,4000).catch(()=>child.kill());}}
async function powershell(code){return exec(ps,['-NoProfile','-NonInteractive','-EncodedCommand',Buffer.from("$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';"+code+';exit 0','utf16le').toString('base64')],{timeout:30000,windowsHide:true,env:{...process.env,TMP:resolve('tmp'),TEMP:resolve('tmp')}});}
function append(entry,existingTarget=entry.target){return {sessionId:entry.registration.sessionId,tabId:randomUUID(),operationId:randomUUID(),cwd,shellId:'pwsh',shellExecutable:pwsh,windowName:`shellfox-${entry.registration.sessionId}`,titleMarker:'',existingTarget};}
async function refusal(input){input.titleMarker=`SHELLFOX:${input.sessionId}:${input.tabId}`;const result=await request('launch',input);assert(!result.ok);assert.equal(result.error.code,'UNSUPPORTED');return result;}
const report={verdict:'incomplete',shells:[],refusals:[],checks:[],registry:'not touched',installer:'not run'};
async function cleanup(entries,file){await writeFile(file,JSON.stringify({shells:entries,cleanup:entries.map(s=>({shell:s.registration.shell}))},null,2));await powershell(`& ${literal(resolve('tests/windows/cleanup-gate.ps1'))} -ReportPath ${literal(file)}`);}
async function scopedWindows(){
 const prefixes=report.shells.map(s=>s.target.markerPrefix);
 const data=Buffer.from(JSON.stringify(prefixes)).toString('base64');
 const code=`Add-Type 'using System;using System.Text;using System.Runtime.InteropServices;public static class ScopeWindows {public delegate bool E(IntPtr h,IntPtr p);[DllImport("user32.dll")]public static extern bool EnumWindows(E f,IntPtr p);[DllImport("user32.dll",CharSet=CharSet.Unicode)]public static extern int GetWindowText(IntPtr h,StringBuilder b,int n);}';$prefixes=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('${data}'))|ConvertFrom-Json;$found=New-Object 'System.Collections.Generic.List[string]';[ScopeWindows]::EnumWindows({param($h,$p)$b=New-Object Text.StringBuilder 2048;[ScopeWindows]::GetWindowText($h,$b,2048)|Out-Null;foreach($prefix in $prefixes){if($b.ToString().StartsWith($prefix)){$found.Add($h.ToInt64().ToString());break}};return $true},[IntPtr]::Zero)|Out-Null;ConvertTo-Json -InputObject @($found.ToArray()) -Compress`;
 return JSON.parse((await powershell(code)).stdout.trim());
}
try{
 start();const probe=await request('initialize',init);assert(probe.ok && probe.value.available);assert.equal(probe.value.capabilities.addTab,false);report.probe=probe.value;
 for(let s=0;s<2;s++){
  const sessionId=randomUUID(),tabId=randomUUID(),operationId=randomUUID();
  const launch={sessionId,tabId,operationId,cwd,shellId:s===0?'pwsh':'windows-powershell',shellExecutable:s===0?pwsh:ps,windowName:`shellfox-${sessionId}`,titleMarker:`SHELLFOX:${sessionId}:${tabId}`,existingTarget:null};
  const receipt=await request('launch',launch);assert(receipt.ok);
  const event=await until(()=>events.find(e=>e.type==='registered' && e.registration.operationId===operationId));
  // Record before assertions so partial binding failures remain recoverable.
  report.shells.push({registration:event.registration,target:event.target,receipt});assert(event.target);
 }
 assert.notEqual(report.shells[0].target.hwnd,report.shells[1].target.hwnd);
 report.checks.push('Two independent initial windows, same metacharacter CWD, both PowerShell versions. addTab is honestly false.');
 const tabs=report.shells.map(s=>({sessionId:s.registration.sessionId,tabId:s.registration.tabId,registration:s.registration}));
 assert((await request('setWatch',{tabs,rules:[]})).ok);
 const before=await scopedWindows();assert.equal(before.length,2);
 const ticketCountBefore=(await readdir(join(init.userDataDir,'launch-tickets'))).length;
 for(const entry of report.shells){for(let t=0;t<2;t++)report.refusals.push(await refusal(append(entry)));}
 await pause(600);
 assert.deepEqual((await scopedWindows()).sort(),before.sort());
 assert.equal(report.shells.length,events.filter(e=>e.type==='registered').length);
 assert.equal((await readdir(join(init.userDataDir,'launch-tickets'))).length,ticketCountBefore);
 report.checks.push('Four direct append attempts refused before tickets/dispatch; no third HWND or new registration.');
 for(const entry of report.shells){const focus=await request('focus',entry.target);assert(focus.ok || focus.error.code==='FOCUS_DENIED');}
 report.checks.push('Existing window focus remains best effort and does not dispatch.');
 await cleanup([report.shells[0]],join(root,'closed-first.json'));
 await pause(300);
 report.refusals.push(await refusal(append(report.shells[0])));
 assert.equal((await scopedWindows()).length,1);
 report.checks.push('An append to an actually destroyed HWND was refused without creating a replacement.');
 await stop();start();assert((await request('initialize',init)).ok);
 // No watch or verified target is supplied here. The durable intent, not cached HWND state,
 // must reject an existingTarget:null bypass after manager/helper restart and window closure.
 report.refusals.push(await refusal(append(report.shells[0],null)));
 assert.equal((await scopedWindows()).length,1);
 const second=report.shells[1];assert((await request('verifyTarget',second.target)).ok);
 assert((await request('setWatch',{tabs:[tabs[1]],rules:[]})).ok);
 report.refusals.push(await refusal(append(second)));
 report.checks.push('Restart plus null-target bypass rejected by durable reservation; surviving window still verifies.');
 report.verdict='fail-safe-routing-proven-not-multitab-acceptance';
}catch(e){report.verdict='blocked';report.error=String(e.stack??e);process.exitCode=1;}
finally{
 await stop();
 try{if(report.shells.length)await cleanup(report.shells,join(root,'cleanup.json'));report.cleanup='only recorded roots and UUID tabs closed';}
 catch(e){report.cleanupError=String(e);report.verdict='cleanup-blocked';process.exitCode=1;}
 await writeFile(join(root,'result.json'),JSON.stringify(report,null,2));
 console.log(JSON.stringify({verdict:report.verdict,checks:report.checks.length,report:join(root,'result.json'),error:report.error,cleanupError:report.cleanupError}));
}
