import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {execFileSync} from 'node:child_process';
import path from 'node:path';
import {root,dimensions,sha256,json,native,launchOwned,closeOwned} from './electron-parity-common.mjs';
import {runStates} from './electron-parity-states.mjs';
const label=process.argv[2];
if(!label||!/^[a-z0-9][a-z0-9-]{0,80}$/.test(label))throw new Error('Usage: node scripts/electron-parity-capture.mjs UNIQUE-BASELINE-LABEL');
const output=path.join(root,'tests/visual-parity/baselines/windows-x64-dpi96',label);
await mkdir(output,{recursive:false});
const context=await launchOwned();
const {app,page,identity,stamp}=context;
const states=[];
const actions=[];
async function capture(name) {
  await page.mouse.move(1190,750);
  await page.evaluate(()=>document.fonts.ready);
  const base=path.join(output,name);
  await page.screenshot({path:base+'.renderer.png',animations:'allow',caret:'initial'});
  const geometry=await page.evaluate(()=>{
    const properties=['display','gridTemplateColumns','width','height','padding','gap','fontFamily','fontSize','fontWeight','lineHeight','color','backgroundColor','border','borderRadius','boxShadow','opacity'];
    const selectors=[':root','.app-shell','.sidebar','.sidebar-toolbar','.sidebar-resizer','.session-section-toggle','.session-row','.session-title','.workspace','.terminal-tab-strip','.terminal-tabs','.terminal-tab-item','.terminal-host','.terminal-surface','.settings-page','.settings-header','.settings-tabs','.modal','.modal > header','.modal h2','.directory-form','.directory-machine','.directory-line','.directory-hint','.context-menu','.file-sidebar','.file-toolbar','button','input','select','textarea'];
    const rect=element=>{const r=element.getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height};};
    return {viewport:{width:innerWidth,height:innerHeight,dpr:devicePixelRatio},title:document.title,
      cssVariables:Object.fromEntries(Array.from(getComputedStyle(document.documentElement)).filter(p=>p.startsWith('--')).map(p=>[p,getComputedStyle(document.documentElement).getPropertyValue(p).trim()])),
      elements:selectors.flatMap(selector=>Array.from(document.querySelectorAll(selector)).filter(e=>e.getClientRects().length).map(element=>({selector,tag:element.tagName,name:element.getAttribute('aria-label')||element.getAttribute('title')||element.textContent.slice(0,100),rect:rect(element),styles:Object.fromEntries(properties.map(p=>[p,getComputedStyle(element)[p]])),svg:Array.from(element.querySelectorAll('svg')).map(svg=>svg.outerHTML)})))};
  });
  await json(base+'.geometry.json',geometry);
  const window=native('capture',identity,base);
  await json(base+'.window.json',window);
  const files={};
  for(const kind of ['renderer','window','client'])files[kind]=sha256(await readFile(base+'.'+kind+'.png'));
  states.push({name,files,viewport:geometry.viewport,window});
  await json(path.join(output,'manifest.json'),{format:1,status:'recorded-not-parity-accepted',engine:'actual Electron BrowserWindow',platform:process.platform,arch:process.arch,dimensions,identity,source:stamp,git:execFileSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8',timeout:10000}).trim(),states,actions,policy:{shell:'actual local PowerShell with safety-only -NoProfile, private HOME/AppData and fixed prompt/output fixture; no presentation/CSS changes',pixelTolerance:0,masks:[],animations:'unaltered',terminalContent:'required, never masked',dynamicPolicy:'No user-approved dynamic exclusions. Repeated capture must prove determinism before acceptance.'}});
  console.log('CAPTURE '+base);
}
try {
  await runStates(context,capture,actions);
  await json(path.join(output,'interactions.json'),actions);
} finally {await closeOwned(context);}
const completed=JSON.parse(await readFile(path.join(output,'manifest.json'),'utf8'));
await json(path.join(output,'manifest.json'),{...completed,captureComplete:true,cleanup:{confirmed:true,method:'generation-scoped terminal closeTab, owned ElectronApplication.close, own isolated test-registry cleanup',dataRemoved:context.run}});
console.log(JSON.stringify({output,states:states.map(s=>s.name)},null,2));
