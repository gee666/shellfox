import { test, expect, _electron } from '@playwright/test';
import path from 'node:path';
import { mkdir, writeFile } from 'node:fs/promises';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { randomUUID } from 'node:crypto';
import { root, scratch, env, testMain, snapshot, value } from '../../tests/fixtures/electron';
const exec=promisify(execFile), literal=(s:string)=>"'"+s.replaceAll("'","''")+"'";
const ps=path.join(process.env.SystemRoot!,'System32/WindowsPowerShell/v1.0/powershell.exe');
const powershell=(code:string)=>exec(ps,['-NoProfile','-NonInteractive','-EncodedCommand',Buffer.from("$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';"+code+';exit 0','utf16le').toString('base64')],{timeout:30000,windowsHide:true});

test('real manager gesture delegates verified focus and bundled-pi cmd/node descendants show running',async()=>{
 expect(process.env.SHELLFOX_WINDOWS_GUI).toBe('1');expect(process.platform).toBe('win32');
 const dir=await scratch('focus-tracking-real'), shellDir=path.join(dir,'shell');await mkdir(shellDir);
 const agent=path.join(dir,'node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js');await mkdir(path.dirname(agent),{recursive:true});await writeFile(agent,'setInterval(() => {}, 1000);\n');
 const command=path.join(dir,'representative-pi.cmd');await writeFile(command,`@"${process.execPath}" "${agent}"\r\n`);
 const mailbox=path.join(dir,'run-agent');
 await writeFile(path.join(shellDir,'bootstrap.ps1'),`param([string]$TicketPath)\n& ${literal(path.join(root,'resources/shell/bootstrap.ps1'))} -TicketPath $TicketPath\nwhile(![IO.File]::Exists(${literal(mailbox)})){Start-Sleep -Milliseconds 100}\n& ${literal(command)}\n`);
 const app=await _electron.launch({cwd:root,env:{...env(),SHELLFOX_TEST_SHELL_DIR:shellDir},args:[testMain,'--test-user-data',path.join(dir,'data'),'--test-backend','real'],timeout:30000});const page=await app.firstWindow();await page.waitForFunction(()=>!!window.shellfox);
 let agentIdentity:any;let records:any[]=[];
 const evidence:any={fixture:'harmless interval at installed pi bundle layout, cmd wrapper; no billable agent',registry:'not touched'};
 try{
  const s=value(await page.evaluate(cwd=>window.shellfox.createSession({cwd,requestId:crypto.randomUUID(),title:'Focus and tracking fixture'}),dir));
  await expect.poll(async()=>(await snapshot(page)).sessions[0]?.tabs[0]?.lifecycle,{timeout:20000}).toBe('open');
  records=await app.evaluate(()=>{const r=(globalThis as any).__shellfoxTest.repository;return r.tabs().filter((t:any)=>t.registration).map((t:any)=>({registration:t.registration,target:r.session(t.sessionId).binding.target}));});
  evidence.registration=records[0];
  const legacy=await page.evaluate(()=>window.shellfox.getSnapshot());const settings=value(legacy).settings;
  const oldScripts=['@earendil-works/pi-coding-agent/dist/cli.js','@mariozechner/pi-coding-agent/dist/cli.js'];
  value(await page.evaluate(input=>window.shellfox.saveSettings(input),{...settings,processRules:settings.processRules.map(r=>r.label==='Pi Node launcher'?{...r,scriptPathSuffixes:oldScripts}:r)}));
  await writeFile(mailbox,'run');
  await expect.poll(async()=>(await snapshot(page)).sessions[0]?.status,{timeout:15000}).toBe('waiting');
  // Verify the actual owned Node process exists, not merely an empty waiting terminal.
  const query=`$root=${records[0].registration.shell.pid};$parent=@(Get-CimInstance Win32_Process -Filter "ParentProcessId=$root"|ForEach-Object{[int]$_.ProcessId});@(Get-CimInstance Win32_Process|Where-Object{$parent -contains [int]$_.ParentProcessId -and $_.Name -eq 'node.exe' -and $_.CommandLine.Contains(${literal(agent)})}|ForEach-Object{@{pid=[int]$_.ProcessId;startTime=(Get-Process -Id $_.ProcessId).StartTime.ToFileTimeUtc().ToString()}})|ConvertTo-Json -Compress`;
  await expect.poll(async()=>{const out=(await powershell(query)).stdout.trim();if(out){const value=JSON.parse(out);agentIdentity=Array.isArray(value)?value[0]:value;}return !!agentIdentity;},{timeout:15000}).toBe(true);
  evidence.beforeFix={status:(await snapshot(page)).sessions[0].status,agentIdentity};
  value(await page.evaluate(input=>window.shellfox.saveSettings(input),settings));
  await expect.poll(async()=>(await snapshot(page)).sessions[0]?.counts.agents,{timeout:15000}).toBe(1);
  await expect.poll(async()=>(await snapshot(page)).sessions[0]?.status,{timeout:15000}).toBe('running');
  // Simulate a persisted earlier denial. Only an observed successful focus may clear it.
  await app.evaluate((_,id)=>{const r=(globalThis as any).__shellfoxTest.repository;const s=r.session(id);s.error={code:'FOCUS_DENIED',message:'Prior foreground request denied',retryable:true};r.saveSession(s);(globalThis as any).__shellfoxTest.service.applyEvent({type:'observations',items:[]});},s.id);
  const manager=await app.evaluate(({BrowserWindow})=>{const w=BrowserWindow.getAllWindows()[0]!;w.show();w.setAlwaysOnTop(true);return {hwnd:w.getNativeWindowHandle().readBigUInt64LE().toString(),bounds:w.getBounds(),pid:process.pid};});
  // Test input only, not a production focus workaround. Click the exact owned title bar
  // after checking WindowFromPoint/owner PID; never send arbitrary keys or touch other windows.
  await powershell(`Add-Type 'using System;using System.Runtime.InteropServices;public static class OwnedClick { [StructLayout(LayoutKind.Sequential)] public struct P { public int X,Y; } [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(P p); [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h,uint f); [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p); [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y); [DllImport("user32.dll")] public static extern void mouse_event(uint f,uint x,uint y,uint d,UIntPtr e); }';$point=New-Object OwnedClick+P;$point.X=${manager.bounds.x+100};$point.Y=${manager.bounds.y+14};$root=[OwnedClick]::GetAncestor([OwnedClick]::WindowFromPoint($point),2);[uint32]$owner=0;[OwnedClick]::GetWindowThreadProcessId($root,[ref]$owner)|Out-Null;if($root.ToInt64().ToString() -ne '${manager.hwnd}' -or $owner -ne ${manager.pid}){throw 'Owned point is covered; no input sent'};[OwnedClick]::SetCursorPos($point.X,$point.Y)|Out-Null;[OwnedClick]::mouse_event(2,0,0,0,[UIntPtr]::Zero);[OwnedClick]::mouse_event(4,0,0,0,[UIntPtr]::Zero)`);
  await app.evaluate(({BrowserWindow})=>BrowserWindow.getAllWindows()[0]!.setAlwaysOnTop(false));
  const before=(await powershell(`Add-Type 'using System;using System.Runtime.InteropServices;public static class Fore { [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p); }';[uint32]$owner=0;$h=[Fore]::GetForegroundWindow();[Fore]::GetWindowThreadProcessId($h,[ref]$owner)|Out-Null;@{hwnd=$h.ToInt64().ToString();pid=$owner}|ConvertTo-Json -Compress`)).stdout.trim();
  evidence.beforeFocus=JSON.parse(before);evidence.managerPid=await app.evaluate(()=>process.pid);
  expect(evidence.beforeFocus.pid).toBe(evidence.managerPid);
  await page.getByRole('button',{name:'Focus window',exact:true}).click();
  await expect.poll(async()=>(await snapshot(page)).sessions[0]?.error,{timeout:10000}).toBeNull();
  await expect.poll(async()=>(await snapshot(page)).sessions[0]?.status,{timeout:15000}).toBe('running');
  evidence.after=(await snapshot(page)).sessions[0];evidence.version=await app.evaluate(({app})=>app.getVersion());
  await writeFile(path.join(dir,'result.json'),JSON.stringify(evidence,null,2));
 }finally{
  if(agentIdentity)await powershell(`$p=Get-Process -Id ${agentIdentity.pid} -ErrorAction SilentlyContinue;if($p -and $p.StartTime.ToFileTimeUtc().ToString() -eq '${agentIdentity.startTime}'){Stop-Process -Id $p.Id}`);
  await page.waitForTimeout(400);
  records=await app.evaluate(()=>{const r=(globalThis as any).__shellfoxTest.repository;return r.tabs().filter((t:any)=>t.registration).map((t:any)=>({registration:t.registration,target:r.session(t.sessionId).binding?.target??null}));});
  await app.close();const report=path.join(dir,'cleanup.json');await writeFile(report,JSON.stringify({shells:records,cleanup:records.map(s=>({shell:s.registration.shell}))},null,2));
  if(records.length)await exec(ps,['-NoProfile','-NonInteractive','-File',path.join(root,'tests/windows/cleanup-gate.ps1'),'-ReportPath',report],{timeout:45000,windowsHide:true});
 }
});
