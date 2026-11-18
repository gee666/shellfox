import { _electron, expect } from '@playwright/test';
import { mkdir,readFile,writeFile,rm } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { randomUUID,createHash } from 'node:crypto';
import { sourceDigest } from './source-digest.mjs';
export const root=path.resolve(import.meta.dirname,'..');
export const fixture=path.join(root,'tests/visual-parity/fixtures/project');
export const dimensions={width:1200,height:760};
export const sha256=data=>createHash('sha256').update(data).digest('hex');
export async function json(file,value) {await writeFile(file,JSON.stringify(value,null,2)+'\n');}
export function native(action,identity,output) {
  if (process.platform!=='win32') throw new Error('Native baseline requires native platform implementation; Windows only here.');
  if ([9484,25904].includes(identity.pid)) throw new Error('Protected user PID.');
  const args=['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',path.join(root,'scripts/electron-parity-window.ps1'),'-Action',action,'-ProcessId',String(identity.pid)];
  if(identity.hwnd)args.push('-WindowHandle',identity.hwnd);
  if(identity.executable)args.push('-Executable',identity.executable);
  if(identity.birth)args.push('-Birth',identity.birth);
  if(output)args.push('-Output',output);
  return JSON.parse(execFileSync('powershell.exe',args,{encoding:'utf8',timeout:20000,windowsHide:true,stdio:['ignore','pipe','pipe']}).trim());
}
export function isolatedEnvironment(home) {
  const allowed=['SystemRoot','SYSTEMROOT','WINDIR','windir','ComSpec','COMSPEC','PATH','Path','PATHEXT','PROGRAMFILES','ProgramFiles','ProgramFiles(x86)','ProgramW6432','PROCESSOR_ARCHITECTURE','NUMBER_OF_PROCESSORS','OS','SYSTEMDRIVE'];
  const env=Object.fromEntries(allowed.filter(k=>process.env[k]!==undefined).map(k=>[k,process.env[k]]));
  Object.assign(env,{HOME:home,USERPROFILE:home,APPDATA:path.join(home,'AppData/Roaming'),LOCALAPPDATA:path.join(home,'AppData/Local'),TEMP:home,TMP:home,
    SHELLFOX_TEST_MODE:'1',SHELLFOX_TEST_VISIBLE:'1',SHELLFOX_TEST_ROOT:root,SHELLFOX_TEST_APP_DATA:path.join(home,'app-data'),SHELLFOX_TEST_LATEST_RELEASE:'',
    XDG_CONFIG_HOME:path.join(home,'.config'),XDG_DATA_HOME:path.join(home,'.local/share'),XDG_CACHE_HOME:path.join(home,'.cache')});
  return env;
}
export function cleanupOwnRegistry(data) {
  const relative=path.relative(path.join(root,'tmp'),data);
  if(!/^electron-parity-[a-f0-9-]+[\\\\/]user-data$/.test(relative))throw new Error('Refusing unrelated registry cleanup.');
  const key=['Software','Shellfox','Tests',sha256(data)].join(String.fromCharCode(92));
  execFileSync('powershell.exe',['-NoProfile','-NonInteractive','-Command',`[Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree('${key}',$false)`],{timeout:15000,windowsHide:true,stdio:['ignore','pipe','pipe']});
}
export async function launchOwned() {
  if(process.platform!=='win32')throw new Error('Capture native macOS/Linux baselines on their own hosts; do not infer them.');
  const dirty=execFileSync('git',['diff','--name-only','HEAD','--','src','tests/fixtures','package.json','vite.config.ts'],{cwd:root,encoding:'utf8',timeout:10000}).trim();
  if(dirty)throw new Error('Baseline requires unchanged committed Electron sources: '+dirty);
  const stamp=JSON.parse(await readFile(path.join(root,'tmp/build-test-source-stamp.json'),'utf8'));
  if(stamp.sourceHash!==sourceDigest(root))throw new Error('Build stamp is stale; run the standard build:test.');
  const run=path.join(root,'tmp','electron-parity-'+randomUUID());await mkdir(run,{recursive:false});
  const data=path.join(run,'user-data'),home=path.join(run,'home');await mkdir(home,{recursive:true});
  const app=await _electron.launch({cwd:root,env:isolatedEnvironment(home),args:[path.join(root,'tmp/build-test/main/index.cjs'),'--test-user-data',data,'--test-backend','real'],timeout:45000});
  try {
  const page=await app.firstWindow();
  await page.waitForFunction(()=>!!window.shellfox);
  await expect(page.getByRole('main',{name:'Workspace',exact:true})).toBeVisible();
  const owner=await app.evaluate(({app,BrowserWindow,session,screen},dimensions)=>{
    if(app.getPath('userData').includes('Shellfox')&&!app.getPath('userData').includes('electron-parity-'))throw new Error('Unexpected userdata');
    session.defaultSession.webRequest.onBeforeRequest((details,callback)=>callback({cancel:!['file:','data:','about:','shellfox-file:'].some(prefix=>details.url.startsWith(prefix))}));
    const window=BrowserWindow.getAllWindows()[0];
    window.unmaximize();window.setContentSize(dimensions.width,dimensions.height);window.setPosition(64,64);window.webContents.setZoomFactor(1);window.show();window.focus();
    const handle=window.getNativeWindowHandle();
    return {pid:process.pid,hwnd:(handle.length===8?handle.readBigUInt64LE():BigInt(handle.readUInt32LE())).toString(),data:app.getPath('userData'),executable:process.execPath,electron:process.versions.electron,chrome:process.versions.chrome,bounds:window.getBounds(),contentBounds:window.getContentBounds(),displays:screen.getAllDisplays()};
  },dimensions);
  if(!owner.data.toLowerCase().startsWith((run+path.sep).toLowerCase()) || [9484,25904].includes(owner.pid) || path.resolve(owner.executable).toLowerCase()!==path.join(root,'node_modules/electron/dist/electron.exe').toLowerCase())throw new Error('Owned Electron identity mismatch.');
  owner.launcherPid=app.process().pid;
  const identity={...owner,...native('identity',owner)};
  if(identity.dpi!==96)throw new Error('Current native DPI is not 96; create a separately labelled DPI baseline instead of rescaling pixels.');
  await app.evaluate(() => {
    const hook=Reflect.get(globalThis,'__shellfoxTest');
    if(!hook?.backend?.getProfiles||!hook.service||!hook.repository)throw new Error('Actual embedded test service unavailable.');
    // Safety fixture only: never execute user PowerShell profiles or load auth.
    for(const profile of hook.backend.profiles.profiles) {
      if(profile.environment==='local' && !profile.elevation && /(?:pwsh|powershell)(?:\.exe)?$/i.test(profile.executable)) {
        if(!profile.args.includes('-NoProfile'))profile.args.push('-NoProfile');
      }
    }
  });
  await page.evaluate(()=>document.fonts.ready);
  return {app,page,run,identity,stamp};
  } catch(error) {await app.close();cleanupOwnRegistry(data);await rm(run,{recursive:true,force:true});throw error;}
}
export async function closeOwned(context) {
  try {
    const actual=await context.app.evaluate(({app})=>({pid:process.pid,data:app.getPath('userData')}));
    if(actual.pid!==context.identity.pid||!actual.data.toLowerCase().startsWith((context.run+path.sep).toLowerCase()))throw new Error('Refusing to close identity mismatch.');
    native('identity',context.identity);
    await context.page.evaluate(async()=>{
      const snapshot=await window.shellfox.getSnapshot();
      if(!snapshot.ok)throw new Error('Owned snapshot unavailable during cleanup.');
      for(const session of snapshot.value.sessions)for(const tab of session.tabs) {
        if(tab.terminalKind==='embedded'&&tab.lifecycle!=='closed'&&tab.generation) {
          const closed=await window.shellfox.closeTab({tabId:tab.id,generation:tab.generation});
          if(!closed.ok)throw new Error('Owned terminal cleanup was not confirmed.');
        }
      }
    });
    await context.app.close();
    cleanupOwnRegistry(actual.data);
    await rm(context.run,{recursive:true,force:true});
  } catch(error) { throw new Error('Owned app cleanup failed; retained isolated directory '+context.run,{cause:error}); }
}
