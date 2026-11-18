// Private008 refusal and exact evidence checks, not ISO/source equivalence.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,mkdtempSync,rmSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {join} from 'node:path';
const root='crates/ghostty-vt-sys/build-support/patches/terminal-profile/';
const dir=process.env.SHELLFOX_008_CANDIDATE_DIR??root+'prototype/native-002-p4-008';
const m=JSON.parse(readFileSync(dir+'/checkpoint.json')),hash=b=>createHash('sha256').update(b).digest('hex');
test('008 fresh55-source replacement and immutable007/negative/header evidence bind together',()=>{
 assert.equal(hash(readFileSync(dir+'/'+m.patch)),m.sha256);assert.equal(m.source_files.length,55);
 assert.equal(hash(Buffer.from(m.source_files.map(f=>`${f.path}\0${f.sha256}\n`).join(''))),m.source_sha256);
 assert.equal(m.complete_replacement_after_exact_prerequisites_not_stacked_on007,true);assert.equal(m.validation.fresh_reproduction.all_compiled_tree_files,1115);
 assert.equal(hash(readFileSync(root+m.predecessor.path)),m.predecessor.sha256);
 assert.equal(hash(readFileSync(root+'prototype/p4-008-plan/before-007-001/review-001.json')),m.validation.negative007.receipt_sha256);
 const old=JSON.parse(readFileSync(root+'prototype/native-002-p4-007/checkpoint.json'));for(const f of m.source_files.filter(f=>f.path.startsWith('include/')))assert.equal(f.sha256,old.source_files.find(p=>p.path===f.path)?.sha256);
});
test('actual source SPA EPA ordinaryEL/ED and hiddenALT qualification regenerates unchanged',()=>{
 const tmp=mkdtempSync('tmp/ghostty-profile/oracle008-audit-');try{
  const out=join(tmp,'oracle.json'),r=spawnSync(process.execPath,[root+'tools/p4-008-oracle.mjs',out],{encoding:'utf8',timeout:90000});assert.equal(r.status,0,r.stderr);
  const bytes=readFileSync(out),o=JSON.parse(bytes);assert.equal(hash(bytes),m.source_files.find(f=>f.path==='src/terminal/shellfox_state3_008_cases.json').sha256);assert.deepEqual(o.summary,{cases:12,cuts:65});
  const el=o.cases.find(c=>c.name==='primary-ISO-EL-DECSTR-persistentA');assert.equal(el.steps[0].expected.primary.rows[0].cells[0].protected,true);assert.equal(el.steps[2].expected.primary.rows[0].cells[0].text,'');assert.equal(el.steps[3].expected.currentBG,0);
  const epa=o.cases.find(c=>c.name==='EPA-does-not-reset-native-mode');assert.equal(epa.steps[1].expected.currentBG,epa.steps[0].expected.currentBG);assert.equal(epa.steps[2].expected.currentBG,epa.steps[0].expected.currentBG,'sourceSPA/EPA fallback changes no current attrs');
  const alt=o.cases.find(c=>c.name==='ALT-hidden-ISO-ED-qualification');assert.equal(alt.steps[1].expected.cols,4);assert.equal(alt.steps[4].expected.alternate.rows[0].cells.length,6);assert.equal(alt.steps[4].expected.alternate.rows[0].cells[0].text,'');
 }finally{rmSync(tmp,{recursive:true,force:true});}
});
test('008 keeps26 measured failures unnormalized but admits none after47 ISO-latched cuts',()=>{
 const before=JSON.parse(readFileSync(dir+'/negative007.json')),live=JSON.parse(readFileSync(dir+'/p4008-iso.json'));
 assert.equal(before.false_admission_cuts,14);assert.equal(before.coherent_current_D2_pass_bad_body_admitted_cuts,13);
 assert.equal(live.cuts,65);assert.equal(live.source_mismatch_cuts,26);assert.equal(live.false_admission_cuts,0);assert.equal(live.d2_pass_bad_body_admitted_cuts,0);
 assert.deepEqual(live.results.reduce((a,x)=>(a[x.admission]=(a[x.admission]??0)+1,a),{}),{success:18,StateUnknown:47});
 for(let i=0;i<65;i++){const a=before.results[i],b=live.results[i];assert.equal(a.name,b.name);assert.equal(a.cut,b.cut);for(const k of ['body_equal','copy_equal','current_coherent','context_equal','source_shadow_equal','source_records_equal','cell_mismatches','primary_mode','alternate_mode','protection_unproved','d2_pass'])assert.equal(a[k],b[k],`${a.name} ${a.cut} ${k}`);}
 assert.equal(m.behavior.source_equivalent_ISO_or_protection,false);assert.equal(m.validation.guard_tests.no_allocation_work_mutation_or_flag_reset,true);assert.ok(m.HOLD.some(s=>s.includes('public C')));
 for(const f of m.evidence_files)assert.equal(hash(readFileSync(dir+'/'+f.path)),f.sha256,f.path);
});
