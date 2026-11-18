// Private007 evidence checks. A refusal pass is NOT DECALN effect equivalence.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,mkdtempSync,rmSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {join} from 'node:path';
const root='crates/ghostty-vt-sys/build-support/patches/terminal-profile/';
const dir=process.env.SHELLFOX_007_CANDIDATE_DIR??root+'prototype/native-002-p4-007';
const m=JSON.parse(readFileSync(dir+'/checkpoint.json')),hash=b=>createHash('sha256').update(b).digest('hex');
test('007 fresh51-file replacement preserves006 and negative-before evidence',()=>{
 assert.equal(hash(readFileSync(dir+'/'+m.patch)),m.sha256);assert.equal(m.source_files.length,51);
 assert.equal(hash(Buffer.from(m.source_files.map(f=>`${f.path}\0${f.sha256}\n`).join(''))),m.source_sha256);
 assert.equal(m.complete_replacement_after_exact_prerequisites_not_stacked_on006,true);
 assert.equal(m.validation.fresh_reproduction.all_compiled_tree_files,1111);
 assert.equal(hash(readFileSync(root+m.predecessor.path)),m.predecessor.sha256);
 const old=JSON.parse(readFileSync(root+'prototype/native-002-p4-006/checkpoint.json'));
 for(const f of m.source_files.filter(f=>f.path.startsWith('include/')))assert.equal(f.sha256,old.source_files.find(p=>p.path===f.path)?.sha256);
 assert.equal(hash(readFileSync(root+'prototype/p4-007-plan/before-006-001/review-001.json')),m.validation.negative006.receipt_sha256);
});
test('actual source oracle proves retained style protection margins origin hidden-backing qualifications',()=>{
 const tmp=mkdtempSync('tmp/ghostty-profile/oracle007-audit-');try{
  const out=join(tmp,'oracle.json'),r=spawnSync(process.execPath,[root+'tools/p4-007-oracle.mjs',out],{encoding:'utf8',timeout:90000});assert.equal(r.status,0,r.stderr);
  const bytes=readFileSync(out),o=JSON.parse(bytes);assert.equal(hash(bytes),m.source_files.find(f=>f.path==='src/terminal/shellfox_state3_007_cases.json').sha256);assert.deepEqual(o.summary,{cases:12,cuts:45});
  const bold=o.cases.find(c=>c.name==='bold-red-then-coherent-SGR0');assert.equal(bold.steps[2].expected.currentFG,0);assert.equal(bold.steps[2].expected.primary.rows[0].cells[0].rawFG,0x09000001);
  const protectedCase=o.cases.find(c=>c.name==='protected-E-current-already-coherent');assert.equal(protectedCase.steps[1].expected.primary.rows[0].cells[0].protected,true);
  const origin=o.cases.find(c=>c.name==='margins-and-origin-fill-start').steps[1].expected;assert.equal(origin.origin,true);assert.equal(origin.primary.scrollTop,1);assert.equal(origin.primary.scrollBottom,2);assert.equal(origin.primary.y,1);assert.equal(origin.primary.rows[0].cells[0].text,'a');
  const alt=o.cases.find(c=>c.name==='ALT-hidden-backing-styled').steps[2].expected;assert.equal(alt.cols,4);assert.equal(alt.alternate.rows[0].cells.length,6);
 }finally{rmSync(tmp,{recursive:true,force:true});}
});
test('007 zero false admissions do not relabel the same28 measured source failures',()=>{
 const before=JSON.parse(readFileSync(dir+'/negative006.json')),live=JSON.parse(readFileSync(dir+'/p4007-decaln.json'));
 assert.equal(before.false_admission_cuts,27);assert.equal(before.coherent_current_but_bad_body_admitted_cuts,18);
 assert.equal(live.cuts,45);assert.equal(live.source_mismatch_cuts,28);assert.equal(live.false_admission_cuts,0);assert.equal(live.coherent_current_but_bad_body_admitted_cuts,0);
 assert.deepEqual(live.results.reduce((a,x)=>(a[x.admission]=(a[x.admission]??0)+1,a),{}),{success:13,StateUnknown:30,Unsupported:2});
 for(let i=0;i<45;i++){const a=before.results[i],b=live.results[i];assert.equal(a.name,b.name);assert.equal(a.cut,b.cut);for(const k of ['body_equal','copy_equal','context_equal','current_coherent','source_shadow_equal','source_records_equal','cell_mismatches'])assert.equal(a[k],b[k],`${a.name} cut${a.cut} ${k}`);}
 assert.equal(m.behavior.source_equivalent_DECALN,false);assert.equal(m.validation.real_style_failure.actual_failing_GPA_OutOfMemory,true);assert.ok(m.HOLD.some(s=>s.includes('public C')));
 for(const f of m.evidence_files)assert.equal(hash(readFileSync(dir+'/'+f.path)),f.sha256,f.path);
});
