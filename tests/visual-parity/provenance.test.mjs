import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile,readdir} from 'node:fs/promises';
import path from 'node:path';
import {execFileSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
const root=fileURLToPath(new URL('../../',import.meta.url));
const baseline=path.join(root,'tests/visual-parity/baselines/windows-x64-dpi96/electron-reference-final');
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');

test('canonical Electron screenshots preserve provenance and complete owned cleanup',async()=>{
  const manifest=JSON.parse(await readFile(path.join(baseline,'manifest.json'),'utf8'));
  assert.equal(manifest.engine,'actual Electron BrowserWindow');
  assert.equal(manifest.captureComplete,true);assert.equal(manifest.cleanup.confirmed,true);
  assert.equal(manifest.identity.dpi,96);assert.equal(manifest.states.length,25);
  assert.deepEqual(manifest.policy.masks,[]);assert.equal(manifest.policy.pixelTolerance,0);
  for(const state of manifest.states)for(const kind of ['window','client','renderer']){
    assert.equal(hash(await readFile(path.join(baseline,state.name+'.'+kind+'.png'))),state.files[kind]);
  }
});

test('historical TypeScript snapshots are inert bytes with original path/SHA, not compiler inputs',async()=>{
  const directory=path.join(baseline,'reference');
  const inventory=JSON.parse(await readFile(path.join(directory,'inventory.json'),'utf8'));
  assert.equal(inventory.files.length,19);
  for(const record of inventory.files){
    assert.ok(record.originalPath);assert.ok(record.artifactPath);
    if(/\.tsx?$/.test(record.originalPath))assert.equal(record.artifactPath,record.originalPath+'.source');
    assert.equal(hash(await readFile(path.join(directory,record.artifactPath))),record.sha256);
  }
  async function inspect(dir){for(const item of await readdir(dir,{withFileTypes:true})){if(item.isDirectory())await inspect(path.join(dir,item.name));else assert.ok(!/\.tsx?$/.test(item.name),'Captured source became a TypeScript input: '+item.name);}}
  await inspect(directory);
});

test('independent real Electron repeatability requires every client AND native pixel',async()=>{
  const report=JSON.parse(await readFile(path.join(root,'tests/visual-parity/reports/final-repeatability-hash-checked/comparison.json'),'utf8'));
  assert.equal(report.pass,true);assert.equal(report.results.length,50);
  for(const result of report.results){assert.equal(result.equal,true);assert.equal(result.changedPixels,0);assert.equal(result.dimensionsEqual,true);}
});

test('protected user PIDs are rejected before any native window/process access',{skip:process.platform!=='win32'},()=>{
  const helper=path.join(root,'scripts/electron-parity-window.ps1');
  for(const pid of [9484,25904]){
    assert.throws(()=>execFileSync('powershell.exe',['-NoProfile','-NonInteractive','-File',helper,'-Action','identity','-ProcessId',String(pid),'-WindowHandle','0'],{timeout:10000,windowsHide:true,stdio:['ignore','pipe','pipe']}),error=>String(error.stderr).includes('Protected user process'));
  }
});
