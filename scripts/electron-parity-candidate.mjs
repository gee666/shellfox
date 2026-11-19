import {spawn} from 'node:child_process';
import {readFile,mkdir,rm,writeFile} from 'node:fs/promises';
import {randomUUID} from 'node:crypto';
import path from 'node:path';
import {DatabaseSync} from 'node:sqlite';
import {root,json,native,isolatedEnvironment,sha256,dimensions} from './electron-parity-common.mjs';
const label=process.argv[2];
if(!label||!/^[a-z0-9][a-z0-9-]{0,80}$/.test(label))throw new Error('Usage: node scripts/electron-parity-candidate.mjs UNIQUE-LABEL');
const output=path.join(root,'tests/visual-parity/baselines/windows-x64-dpi96',label);
await mkdir(output,{recursive:false});
const run=path.join(root,'tmp','electron-parity-candidate-'+randomUUID()),home=path.join(run,'home'),data=path.join(run,'data');
await mkdir(run,{recursive:false});await mkdir(home,{recursive:false});
const executable=path.join(root,'target/debug/shellfox.exe');
const child=spawn(executable,['--data-dir',data],{cwd:root,env:isolatedEnvironment(home),stdio:['ignore','pipe','pipe'],windowsHide:false});
let logs='',identity;
child.stdout.on('data',chunk=>{logs+=chunk;});child.stderr.on('data',chunk=>{logs+=chunk;});
try {
  if(!child.pid||[9484,25904].includes(child.pid))throw new Error('Invalid newly owned child PID.');
  const deadline=Date.now()+30000;
  let lastError;
  while(Date.now()<deadline) {
    if(child.exitCode!==null)throw new Error('Owned candidate exited: '+logs);
    try {identity={pid:child.pid,executable,...native('discover',{pid:child.pid,executable})};break;}
    catch(error){lastError=error;await new Promise(resolve=>setTimeout(resolve,250));}
  }
  if(!identity)throw new Error('Owned candidate window unavailable.',{cause:lastError});
  if(identity.dpi!==96)throw new Error('Candidate DPI differs from baseline; do not rescale.');
  let mismatchRefused=false;
  try {native('identity',{...identity,birth:'1'});}catch {mismatchRefused=true;}
  if(!mismatchRefused)throw new Error('Birth-mismatch guard did not reject.');
  native('configure',identity);
  await new Promise(resolve=>setTimeout(resolve,500));
  const store=new DatabaseSync(path.join(data,'shellfox-rust-v1.sqlite3'),{readOnly:true});
  try {if(store.prepare('SELECT count(*) AS count FROM sessions').get().count!==0)throw new Error('Unexpected candidate state; refuse capture or input.');}finally{store.close();}
  const window=native('capture',identity,path.join(output,'01-empty'));
  const files={};
  for(const kind of ['client','window'])files[kind]=sha256(await readFile(path.join(output,'01-empty.'+kind+'.png')));
  await json(path.join(output,'manifest.json'),{format:1,status:'candidate-only-not-parity-accepted',engine:'actual GPUI owned window',platform:process.platform,arch:process.arch,dimensions,identity:{...identity,...window},executableSha256:sha256(await readFile(executable)),lockSha256:sha256(await readFile(path.join(root,'Cargo.lock'))),birthMismatchGuard:mismatchRefused,states:[{name:'01-empty',files,window}],missingStates:'All nonempty/menu/dialog/settings/editor states remain required, not skipped by comparator.',policy:{pixelTolerance:0,masks:[],animations:'unaltered',terminalContent:'required, never masked'}});
  console.log(JSON.stringify({output,identity,window},null,2));
} finally {
  await writeFile(path.join(output,'owned-process.log'),logs);
  if(identity&&child.exitCode===null)native('close',identity);
  if(child.exitCode===null&&child.signalCode===null)await new Promise((resolve,reject)=>{
    const timer=setTimeout(()=>reject(new Error('Owned candidate did not confirm exit. No PID kill fallback; retained '+run)),15000);
    child.once('exit',()=>{clearTimeout(timer);resolve();});
    child.once('error',error=>{clearTimeout(timer);reject(error);});
  });
  if(child.exitCode!==null)await rm(run,{recursive:true,force:true});
}
