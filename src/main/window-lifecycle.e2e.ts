import { _electron, test, expect, type ElectronApplication } from '@playwright/test';
import path from 'node:path';
import { writeFile } from 'node:fs/promises';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { scratch, snapshot, value, root, env, testMain } from '../../tests/fixtures/electron';

async function start(data:string,backend:'fake'|'real'='fake') {
  const app=await _electron.launch({cwd:root,env:{...env(),SHELLFOX_FAKE_SESSION_WINDOWS:backend==='fake'?'1':'0'},args:[testMain,'--test-user-data',data,'--test-backend',backend],timeout:30000});
  const page=await app.firstWindow();await page.waitForFunction(()=>!!window.shellfox);return {app,page};
}
async function records(app:ElectronApplication) {
  return app.evaluate(()=>{const repository=(globalThis as any).__shellfoxTest.repository;return repository.tabs().filter((t:any)=>t.member).map((t:any)=>({registration:t.member.registration,target:t.member.binding.target}));});
}
async function clean(app:ElectronApplication,dir:string,name:string,entries?:any[]) {
  const shells=entries??await records(app);const report=path.join(dir,name+'.json');
  await writeFile(report,JSON.stringify({shells,cleanup:shells.map((s:any)=>({shell:s.registration.shell}))},null,2));
  const executable=path.join(process.env.SystemRoot??'C:\\Windows','System32/WindowsPowerShell/v1.0/powershell.exe');
  await promisify(execFile)(executable,['-NoProfile','-NonInteractive','-File',path.join(root,'tests/windows/cleanup-gate.ps1'),'-ReportPath',report],{timeout:45000,windowsHide:true});
}

test('fake UI click reopens the same closed session once, shows registration instructions and rejects stale closure',async()=>{
  const dir=await scratch('window-ui-fake');const {app,page}=await start(path.join(dir,'data'));
  try {
    const s=value(await page.evaluate(cwd=>window.shellfox.createSession({cwd,requestId:crypto.randomUUID(),title:'Reopen UI'}),dir));
    const old=await app.evaluate((_,id)=>{const b=(globalThis as any).__shellfoxTest.backend;const old=b.bindings.get(id);b.closeWindow(id);return old;},s.id);
    await expect.poll(async()=>(await snapshot(page)).sessions[0].window?.state).toBe('closed');
    await page.getByRole('button',{name:/Reopen UI.*window closed/}).click();
    await expect.poll(async()=>(await snapshot(page)).sessions[0].window?.state).toBe('alive');
    await page.evaluate(id=>Promise.all([window.shellfox.activateSession({sessionId:id}),window.shellfox.activateSession({sessionId:id})]),s.id);
    expect(await app.evaluate(()=>(globalThis as any).__shellfoxTest.backend.reopens.length)).toBe(1);
    expect((await snapshot(page)).sessions[0].id).toBe(s.id);
    await app.evaluate((_,binding)=>(globalThis as any).__shellfoxTest.backend.windowEvent({type:'lost',binding,reason:'stale close'}),old);
    expect((await snapshot(page)).sessions[0].window?.state).toBe('alive');
    await page.getByRole('button',{name:'Register this terminal',exact:true}).click();
    const modal=page.getByRole('dialog',{name:'Register the selected PowerShell tab'});
    await expect(modal).toBeVisible();await expect(modal.getByLabel('Registration command')).toHaveValue(/user''s folder/);
    await expect(modal).toContainText('NOT discovered automatically');
    await app.evaluate(()=>(globalThis as any).__shellfoxTest.backend.registerPrepared(0));
    await modal.getByRole('button',{name:'Done, refresh membership'}).click();
    await expect.poll(async()=>(await snapshot(page)).sessions[0].tabs.length).toBe(3);
    expect(await app.evaluate(()=>(globalThis as any).__shellfoxTest.backend.launches.length)).toBe(2);
  } finally {await app.close();}
});

test('fake confirmed transfer preserves tab identity and SQLite window/member associations on restart',async()=>{
  const dir=await scratch('window-transfer-fake'),data=path.join(dir,'data');let {app,page}=await start(data);let stopped=false;
  try{
    const sessions=await page.evaluate(async cwd=>{
      const a=await window.shellfox.createSession({cwd,requestId:crypto.randomUUID(),title:'Source'});
      const b=await window.shellfox.createSession({cwd,requestId:crypto.randomUUID(),title:'Destination'});return [a,b];
    },dir);const a=value(sessions[0]),b=value(sessions[1]);
    value(await page.evaluate(id=>window.shellfox.prepareSessionRegistration({sessionId:id,shellId:'pwsh'}),b.id));
    await app.evaluate((_,tabId)=>(globalThis as any).__shellfoxTest.backend.registerPrepared(0,tabId),a.tabs[0].id);
    await expect.poll(async()=>(await snapshot(page)).sessions.find(s=>s.id===b.id)?.tabs.length).toBe(2);
    expect((await snapshot(page)).sessions.find(s=>s.id===a.id)?.tabs).toHaveLength(0);
    const before=await app.evaluate(()=>{const r=(globalThis as any).__shellfoxTest.repository;return {sessions:r.sessions().map((s:any)=>({id:s.id,binding:s.binding})),members:r.tabs().map((t:any)=>({id:t.id,sessionId:t.sessionId,member:t.member}))};});
    await app.close();stopped=true;({app,page}=await start(data));stopped=false;
    const after=await app.evaluate(()=>{const r=(globalThis as any).__shellfoxTest.repository;return {sessions:r.sessions().map((s:any)=>({id:s.id,binding:s.binding})),members:r.tabs().map((t:any)=>({id:t.id,sessionId:t.sessionId,member:t.member}))};});
    expect(after).toEqual(before);expect(after.members.find((t:any)=>t.id===a.tabs[0].id)?.sessionId).toBe(b.id);
    expect(await app.evaluate(()=>(globalThis as any).__shellfoxTest.backend.launches.length)).toBe(0);
  }finally{if(!stopped)await app.close();}
});

test('real native window closure then UI click opens one replacement on the same session',async()=>{
  test.skip(process.env.SHELLFOX_WINDOWS_GUI!=='1','Requires explicit permission for test-owned Windows Terminal windows');
  const dir=await scratch('window-click-real');const {app,page}=await start(path.join(dir,'data'),'real');
  try{
    const s=value(await page.evaluate(cwd=>window.shellfox.createSession({cwd,requestId:crypto.randomUUID(),title:'Real reopen fixture'}),dir));
    await expect.poll(async()=>(await snapshot(page)).sessions[0].window?.state,{timeout:20000}).toBe('alive');
    expect((await snapshot(page)).probe.capabilities.addTab).toBe(false);
    await page.waitForTimeout(500);await clean(app,dir,'close-original');
    await expect.poll(async()=>(await snapshot(page)).sessions[0].window?.state,{timeout:15000}).toBe('closed');
    await page.getByRole('button',{name:/Real reopen fixture.*window closed/}).click();
    await expect.poll(async()=>(await snapshot(page)).sessions[0].window?.state,{timeout:20000}).toBe('alive');
    const current=(await snapshot(page)).sessions[0];expect(current.id).toBe(s.id);expect(current.tabs).toHaveLength(2);
    expect(current.tabs[1].id).not.toBe(s.tabs[0].id);
    const ledger=await app.evaluate(()=>{const r=(globalThis as any).__shellfoxTest.repository;return {sessions:r.sessions().length,reopens:r.db.prepare("SELECT count(*) AS n FROM operations WHERE kind='reopen'").get().n};});
    expect(ledger).toEqual({sessions:1,reopens:1});
    await writeFile(path.join(dir,'result.json'),JSON.stringify({sessionId:s.id,tabs:await records(app),behavior:'confirmed window-close click reopened same session; no old commands restored'},null,2));
  }finally{await page.waitForTimeout(500);try{await clean(app,dir,'cleanup');}finally{await app.close();}}
});
