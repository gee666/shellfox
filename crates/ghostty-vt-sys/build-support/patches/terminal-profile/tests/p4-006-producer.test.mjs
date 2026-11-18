// Private006 evidence only. Does not accept public APIs, packing or a codec.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,mkdtempSync,rmSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {join} from 'node:path';
const root='crates/ghostty-vt-sys/build-support/patches/terminal-profile/';
const dir=process.env.SHELLFOX_006_CANDIDATE_DIR??root+'prototype/native-002-p4-006';
const m=JSON.parse(readFileSync(dir+'/checkpoint.json'));
const hash=b=>createHash('sha256').update(b).digest('hex');
test('006 replacement binds fresh47-source composition and preserves immutable005/header ABI',()=>{
 assert.equal(hash(readFileSync(dir+'/'+m.patch)),m.sha256);assert.equal(m.source_files.length,47);
 assert.equal(hash(Buffer.from(m.source_files.map(f=>`${f.path}\0${f.sha256}\n`).join(''))),m.source_sha256);
 assert.equal(m.complete_replacement_after_exact_prerequisites_not_stacked_on005,true);
 assert.equal(m.validation.fresh_reproduction.all_compiled_tree_files,1107);
 assert.equal(hash(readFileSync(root+m.predecessor.path)),m.predecessor.sha256);
 const old=JSON.parse(readFileSync(root+'prototype/native-002-p4-005/checkpoint.json'));
 assert.deepEqual(m.production_delta,['src/terminal/shellfox_state3_ops.zig']);
 assert.equal(m.changed_files_from005.length,5);
 for(const f of m.source_files.filter(f=>f.path.startsWith('include/')))assert.equal(f.sha256,old.source_files.find(p=>p.path===f.path)?.sha256);
});
test('actual shipped source regenerates exact006 no-output/reset URI/copy and refusal evidence',()=>{
 const tmp=mkdtempSync('tmp/ghostty-profile/oracle006-audit-');
 try{
  const out=join(tmp,'oracle.json'),r=spawnSync(process.execPath,[root+'tools/p4-006-oracle.mjs',out],{encoding:'utf8',timeout:90000});assert.equal(r.status,0,r.stderr);
  const bytes=readFileSync(out),o=JSON.parse(bytes);assert.equal(hash(bytes),m.source_files.find(f=>f.path==='src/terminal/shellfox_state3_006_cases.json').sha256);
  assert.deepEqual(o.summary,{cases:15,cuts:58,admitted:32,refused:26});
  const noText=o.cases.find(c=>c.name==='no-output-open-implicit-ST');
  assert.equal(noText.steps[0].expected.primary.copy,'');assert.notEqual(noText.steps[0].expected.currentUri,'');
  assert.equal(noText.steps[1].expected.currentUri,'');assert.equal(noText.steps[1].admission,'success');
  assert.equal(noText.steps[2].expected.primary.rows[0][0].uri,'');assert.equal(noText.steps[2].expected.primary.copy,'B');
  const sgr=o.cases.find(c=>c.name==='SGR0-does-not-close-pending-URI');assert.notEqual(sgr.steps[1].expected.currentUri,'');assert.equal(sgr.steps[1].admission,'Unsupported');
  const history=o.cases.find(c=>c.name==='historical-cell-A-pending-B').steps.at(-1);assert.equal(history.expected.primary.rows[0][0].uri,'https://example.test/A');assert.equal(history.expected.primary.rows[0][1].uri,'');assert.equal(history.expected.primary.copy,'HU');assert.equal(history.admission,'StateUnknown');
 }finally{rmSync(tmp,{recursive:true,force:true});}
});
test('006 evidence retains negative005 failures, real allocation/refcount scope and public holds',()=>{
 const before=JSON.parse(readFileSync(dir+'/before-005.json'));assert.equal(before.unique_failed_groups,5);assert.equal(before.failed_executions,10);assert.equal(before.source_URI_mismatch_cuts,18);
 const actual=JSON.parse(readFileSync(dir+'/p4006-url-reset.json'));assert.equal(actual.native_effect_equivalent_cuts,58);assert.equal(actual.admitted_subset_cuts,32);assert.equal(actual.strict_subset_not_admitted_cuts,26);
 assert.equal(m.validation.real_reopen_GPA.actual_induced_failures,3);assert.equal(m.validation.release_safe.failed,0);assert.equal(m.validation.debug_targeted.failed,0);
 assert.equal(m.validation.native0.prior_DECSTR_link_retention_runtime_test_passed,true);
 assert.ok(m.HOLD.some(s=>s.includes('format3')));assert.ok(m.HOLD.some(s=>s.includes('public Legacy')));
 for(const f of m.evidence_files)assert.equal(hash(readFileSync(dir+'/'+f.path)),f.sha256,f.path);
});
