// Private immutable candidate/source-evidence checks. No public codec acceptance.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,mkdtempSync,rmSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {join} from 'node:path';
const root='crates/ghostty-vt-sys/build-support/patches/terminal-profile/';
const dir=process.env.SHELLFOX_005_CANDIDATE_DIR??root+'prototype/native-002-p4-005';
const m=JSON.parse(readFileSync(dir+'/checkpoint.json'));
const hash=b=>createHash('sha256').update(b).digest('hex');
test('immutable005 patch and source digest bind exact prerequisite replacement, not004 overwrite',()=>{
 assert.equal(hash(readFileSync(dir+'/'+m.patch)),m.sha256);
 assert.equal(m.source_files.length,44);
 assert.equal(hash(Buffer.from(m.source_files.map(f=>`${f.path}\0${f.sha256}\n`).join(''))),m.source_sha256);
 assert.equal(m.complete_replacement_after_exact_prerequisites_not_stacked_on004,true);
 assert.equal(m.validation.patch_reproduction.entire_compiled_src_include_tree_compared,1104);
 assert.equal(hash(readFileSync(root+m.predecessor.path)),m.predecessor.sha256);
 const old=JSON.parse(readFileSync(root+'prototype/native-002-p4-004/checkpoint.json'));
 for(const f of m.source_files.filter(f=>f.path.startsWith('include/')))assert.equal(f.sha256,old.source_files.find(p=>p.path===f.path)?.sha256,'No new005 header ABI');
});
test('actual unchanged shipped source regenerates the exact005 oracle and retained shrink records',()=>{
 const tmp=mkdtempSync('tmp/ghostty-profile/oracle005-audit-');
 try{
  const out=join(tmp,'oracle.json');
  const result=spawnSync(process.execPath,[root+'tools/p4-005-oracle.mjs','1410',out],{encoding:'utf8',timeout:90000});
  assert.equal(result.status,0,result.stderr);
  const bytes=readFileSync(out),oracle=JSON.parse(bytes);
  assert.equal(hash(bytes),m.source_files.find(p=>p.path==='src/terminal/shellfox_state3_005_cases.json').sha256);
  assert.deepEqual(oracle.summary,{cases:14,cuts:130});
  assert.deepEqual([...new Set(oracle.cases.filter(c=>c.name.startsWith('native-P')).map(c=>c.scrollback))],[1409,1410,1411,2821]);
  const bottom=oracle.cases.find(c=>c.name==='full-history-height-shrink-primary-cursor4');
  assert.equal(bottom.steps[0].expected.primary.savedBufferY,5);
  assert.equal(bottom.steps[1].expected.primary.savedBufferY,3);
  const above=oracle.cases.find(c=>c.name==='full-history-height-shrink-primary-cursor1');
  assert.equal(above.steps[1].expected.primary.savedBufferY,5,'bottom pops are not top trims or a viewport clamp');
  assert.equal(oracle.cases.filter(c=>!c.admit).reduce((n,c)=>n+c.steps.length,0),48);
 }finally{rmSync(tmp,{recursive:true,force:true});}
});
test('receipts preserve004 failures and005 public/format/install holds rather than relabel them',()=>{
 const before=JSON.parse(readFileSync(dir+'/before-004.json'));
 assert.equal(before.oracle_mismatch_cuts,67);assert.equal(before.compiled_summary.failed,3);
 const actual=JSON.parse(readFileSync(dir+'/p4005-boundaries-height-managed.json'));
 assert.equal(actual.native_effect_equivalent_cuts,130);assert.equal(actual.admitted_subset_cuts,82);assert.equal(actual.strict_subset_not_admitted_cuts,48);
 assert.equal(m.validation.release_safe.failed,0);assert.equal(m.validation.debug_full.pass_claim,false);
 assert.ok(m.HOLD.some(s=>s.includes('format3')));assert.ok(m.HOLD.some(s=>s.includes('public Legacy')));
 for(const f of m.evidence_files)assert.equal(hash(readFileSync(dir+'/'+f.path)),f.sha256,f.path);
});
