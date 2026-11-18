import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';
import { project, vendor, verifyInputs } from './archives.mjs';
import { applyExperimental, experimentalBuildArgs, experimentalRecipe, headerPins, patchPins,
  sha256, sourceTreeDigest, verifyExperimentalInputs } from './experimental.mjs';

function scratch() { mkdirSync(join(project,'tmp'),{recursive:true});return mkdtempSync(join(project,'tmp','native002-plan-test-')); }
function patchTool() {
  const windows = join(process.env.ProgramFiles ?? 'C:/Program Files','Git/usr/bin/patch.exe');
  return process.platform === 'win32' && existsSync(windows) ? windows : 'patch';
}

test('only the three exact candidate patches and matching prerequisite checkpoints are accepted', () => {
  const verified=verifyExperimentalInputs();
  assert.deepEqual(verified.map(p=>p.order),[1,2,3]);
  assert.deepEqual(verified.map(p=>p.sha256),patchPins.map(p=>p.sha256));
  assert.throws(()=>verifyExperimentalInputs(path=>{
    const data=readFileSync(path);
    return String(path).endsWith('0001-ansi-export.patch') ? Buffer.concat([data,Buffer.from('\n')]) : data;
  }),/patch SHA-256 mismatch/);
  assert.throws(()=>verifyExperimentalInputs(path=>{
    const data=readFileSync(path);
    if(!String(path).includes('terminal-profile')||!String(path).endsWith('checkpoint.json'))return data;
    const checkpoint=JSON.parse(data);checkpoint.requires.patchSha256='0'.repeat(64);return Buffer.from(JSON.stringify(checkpoint));
  }),/prerequisite mismatch/);
});

test('profile is deterministic, external, and binds target/options/ordered patches/headers', () => {
  const recipe=experimentalRecipe('x86_64-pc-windows-msvc','x86_64-windows-gnu');
  assert.equal(recipe.engineProfile,sha256(recipe.text));
  assert.equal(experimentalRecipe(recipe.rustTarget,recipe.zigTarget).engineProfile,recipe.engineProfile);
  assert.notEqual(experimentalRecipe('aarch64-linux-android','aarch64-linux-android.24').engineProfile,recipe.engineProfile);
  assert(!recipe.text.includes(recipe.engineProfile));
  for(const pin of patchPins)assert(recipe.text.includes(`patch${pin.order} ${pin.sha256}\n`));
  for(const digest of Object.values(headerPins))assert(recipe.text.includes(digest));
  assert.deepEqual(experimentalBuildArgs(recipe),['-Doptimize=ReleaseSafe','-Dversion-string=1.4.1-dev','-Dlib-version-string=1.0.1',`-Dshellfox-engine-profile=${recipe.engineProfile}`,'-Demit-lib-vt','-j1']);
  assert.throws(()=>experimentalRecipe('bad\ntarget','x86_64-windows-gnu'),/Invalid/);
});

test('source fingerprint is location-independent and detects actual content changes', () => {
  const root=scratch();
  try {
    for(const name of ['one','two']) {mkdirSync(join(root,name));writeFileSync(join(root,name,'source.zig'),'hello');}
    assert.equal(sourceTreeDigest(join(root,'one')),sourceTreeDigest(join(root,'two')));
    writeFileSync(join(root,'two','source.zig'),'different');
    assert.notEqual(sourceTreeDigest(join(root,'one')),sourceTreeDigest(join(root,'two')));
  } finally {rmSync(root,{recursive:true,force:true});}
});

test('fresh signed-source extraction composes actual candidates 1 then 2 then 3 with exact resulting headers', () => {
  verifyInputs(['libghostty-vt-1.0.1.tar.gz']);
  const root=scratch();
  try {
    const tar=process.platform==='win32'?join(process.env.SystemRoot ?? 'C:/Windows','System32/tar.exe'):'tar';
    const result=spawnSync(tar,['-xzf',join(vendor,'libghostty-vt-1.0.1.tar.gz'),'-C',root],{encoding:'utf8',timeout:120000});
    assert.equal(result.status,0,result.stderr);
    const source=join(root,'libghostty-vt-1.0.1');
    const records=applyExperimental(source,patchTool());
    assert.deepEqual(records.map(p=>p.sha256),patchPins.map(p=>p.sha256));
    assert.equal(records.length,3);
    const profile=readFileSync(join(source,'src/terminal/shellfox_profile.zig'),'utf8');
    assert.match(profile,/terminal_profile_abi_version: u32 = 1/);
    assert.match(profile,/snapshot_budget_abi_version: u32 = 1/);
    assert.throws(()=>applyExperimental(source,patchTool()),/already patched/);
  } finally {rmSync(root,{recursive:true,force:true});}
});
