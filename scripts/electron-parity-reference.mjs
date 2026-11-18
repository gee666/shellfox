import {readFile,writeFile,mkdir,readdir,copyFile} from 'node:fs/promises';
import {execFileSync} from 'node:child_process';
import path from 'node:path';
import {root,sha256,json} from './electron-parity-common.mjs';
import {sourceDigest} from './source-digest.mjs';
const directory=path.resolve(process.argv[2]||'');
const allowed=path.join(root,'tests/visual-parity/baselines')+path.sep;
if(!directory.startsWith(allowed))throw new Error('Reference bundle must belong to owned baseline fixtures.');
const manifest=JSON.parse(await readFile(path.join(directory,'manifest.json'),'utf8'));
if(manifest.source.sourceHash!==sourceDigest(root))throw new Error('Baseline source no longer matches current Electron source.');
const output=path.join(directory,'reference');await mkdir(output,{recursive:false});
const files=['src/renderer/components.tsx','src/renderer/TerminalPlaceholder.tsx','src/renderer/TerminalTabs.tsx','src/renderer/theme.ts','src/renderer/terminal-surface.ts','src/main/window-state.ts','src/main/index.ts','resources/icon/icon.ico','resources/icon/icon-256.png'];
async function styles(relative) {
  for(const entry of await readdir(path.join(root,relative),{withFileTypes:true})){
    const file=path.join(relative,entry.name);
    if(entry.isDirectory())await styles(file);else if(entry.name.endsWith('.css'))files.push(file);
  }
}
await styles('src/renderer');
const inventory=[];
for(const file of files) {
  const artifact=file+(/\.tsx?$/.test(file)?'.source':'');
  const target=path.join(output,artifact);await mkdir(path.dirname(target),{recursive:true});
  await copyFile(path.join(root,file),target);
  inventory.push({originalPath:file.replaceAll('\\','/'),artifactPath:artifact.replaceAll('\\','/'),sha256:sha256(await readFile(target))});
}
const machine=JSON.parse(execFileSync('powershell.exe',['-NoProfile','-NonInteractive','-Command',
  "Add-Type -AssemblyName System.Drawing; $os=Get-CimInstance Win32_OperatingSystem; $fonts=New-Object Drawing.Text.InstalledFontCollection; @{osVersion=$os.Version;osBuild=$os.BuildNumber;fonts=@($fonts.Families | ForEach-Object {$_.Name} | Sort-Object)} | ConvertTo-Json -Depth 4 -Compress"],{encoding:'utf8',timeout:20000,windowsHide:true}).trim());
machine.fontFiles=[];
for(const file of ['segoeui.ttf','segoeuib.ttf','consola.ttf','consolab.ttf'])machine.fontFiles.push({file,sha256:sha256(await readFile(path.join(process.env.WINDIR||process.env.SystemRoot,'Fonts',file)))});
await json(path.join(output,'inventory.json'),{source:manifest.source,git:manifest.git,files:inventory,machine,rule:'Exact committed source/assets and measured SVG/geometry; not a CSS approximation or replacement UI.'});
console.log(JSON.stringify({output,files:inventory.length,uiFont:machine.fonts.includes('Segoe UI'),terminalCandidates:machine.fonts.filter(font=>['Cascadia Code','Consolas','DejaVu Sans Mono'].includes(font))},null,2));
