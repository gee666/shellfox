import assert from 'node:assert/strict';
import test from 'node:test';
import {readFileSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {compareCursorSourceState} from '../tools/cursor-state-compare.mjs';
const root='crates/ghostty-vt-sys/build-support/patches/terminal-profile/';
const output='tmp/ghostty-profile/cursor-state-v2-review/oracle.json';
let corpus;
test('pre-edit oracle captures actual retained saved/tabs/copy state, not a producer pass',()=>{
 const result=spawnSync(process.execPath,[root+'tools/cursor-state-v2-oracle.mjs',output],{encoding:'utf8',timeout:30000});assert.equal(result.status,0,result.stderr);
 corpus=JSON.parse(readFileSync(output));assert.equal(corpus.cases.length,25);assert.ok(corpus.summary.nonzeroBaseBufferCuts>0);assert.ok(corpus.summary.savedOutsideCurrentRowsBufferCuts>0);assert.ok(corpus.summary.offGridTabBufferCuts>0);
 for(const c of corpus.cases)for(const s of c.steps)compareCursorSourceState(s.expected,structuredClone(s.expected));
 const old=JSON.parse(readFileSync(root+'prototype/cursor-root-review/oracle.json'));assert.equal(old.cases.length,620);assert.equal(old.cases.reduce((n,c)=>n+c.steps.length,0),2880);assert.deepEqual([...new Set(old.cases.flatMap(c=>c.steps.map(s=>s.expected.base)))],[0]);
});
function entry(name){return corpus.cases.find(c=>c.name===name);}
test('history growth does not reconstruct savedBufferY from current base',()=>{
 const c=entry('saved-before-history-growth');assert.equal(c.steps[1].expected.primary.base,2);assert.equal(c.steps[1].expected.primary.savedBufferY,1);assert.equal(c.steps[2].expected.primary.y,0);
 const r=entry('save-before-growth-then-resize-must-not-rebase-old-relativeY');assert.equal(r.steps[2].expected.primary.base,0);assert.equal(r.steps[2].expected.primary.savedBufferY,1);assert.equal(r.steps[3].expected.primary.y,1);
});
test('recycle, resize-option trim and ED3 have different saved-index rules',()=>{
 assert.equal(entry('ordinary-capacity-recycle-keeps-numeric-savedY').steps[1].expected.primary.savedBufferY,4);
 const option=entry('scrollback-option-trim-adjusts-savedY').steps[1].expected.primary,ed=entry('ED3-trim-does-not-adjust-savedY').steps[1].expected.primary;
 assert.equal(option.base,0);assert.equal(ed.base,0);assert.equal(option.savedBufferY,2);assert.equal(ed.savedBufferY,4);assert.ok(ed.savedBufferY>=ed.rows.length);
 const alt=entry('unfilled-ALT-savedX-survives-shrink');assert.equal(alt.steps[2].expected.cols,3);assert.equal(alt.steps[2].expected.alternate.rows.length,0);assert.equal(alt.steps[2].expected.alternate.savedX,5);assert.equal(alt.steps[3].expected.alternate.x,2);assert.equal(alt.steps[3].expected.alternate.savedX,5);
});
test('reverse BS uses reverse mode alone and actual wide early-wrap left edge',()=>{
 for(const name of ['reverse-BS-DECAWMoff-pending','reverse-BS-DECAWMoff-overflow']){const e=entry(name).steps[2].expected;assert.equal(e.wraparound,false);assert.equal(e.reverseWraparound,true);assert.equal(e.primary.x,3);assert.equal(entry(name).steps[3].expected.primary.copy,'ABCX');}
 const e=entry('reverse-BS-DECAWMoff-wide-earlywrap-leftedge');assert.equal(e.steps[0].expected.primary.rows[1].wrapped,true);assert.deepEqual([e.steps[2].expected.primary.x,e.steps[2].expected.primary.y],[2,0]);assert.equal(e.steps[3].expected.primary.copy,'ABX\n中');
});
test('off-grid keys survive and per-buffer tables affect future tabs',()=>{
 assert.deepEqual(entry('HTS-at-pending-survives-growth').steps[1].expected.primary.tabs,[4]);assert.equal(entry('HTS-at-pending-survives-growth').steps[3].expected.primary.x,4);
 assert.deepEqual(entry('HTS-at-overflow-survives-growth').steps[1].expected.primary.tabs,[5]);assert.equal(entry('HTS-at-overflow-survives-growth').steps[3].expected.primary.x,5);
 assert.equal(entry('TBC-at-overflow-must-not-remove-physical3-stop').steps[3].expected.primary.x,3);
 assert.ok(entry('off-grid-tab-stop-retained-on-shrink').steps[1].expected.primary.tabs.includes(10));
 const e=entry('tab-tables-are-per-buffer').steps[1].expected;assert.deepEqual(e.primary.tabs,[2]);assert.deepEqual(e.alternate.tabs,[5]);assert.equal(entry('tab-tables-are-per-buffer').steps[2].expected.primary.x,2);
});
test('new comparator rejects copy loss, invented saved index and truncated/swapped tables',()=>{
 const e=entry('HTS-at-overflow-survives-growth').steps[1].expected;
 for(const change of [a=>{a.primary.copy+=' ';},a=>{a.primary.savedBufferY++;},a=>{a.primary.tabs=[];},a=>{[a.primary.tabs,a.alternate.tabs]=[a.alternate.tabs,a.primary.tabs];},a=>{delete a.primary.copy;}]){const a=structuredClone(e);change(a);assert.throws(()=>compareCursorSourceState(e,a));}
});
test('frozen P4-003 remains byte-identical and not relabeled',()=>{
 const p=root+'prototype/native-002-p4-003/';const m=JSON.parse(readFileSync(p+'checkpoint.json'));assert.equal(m.sha256,'6be5355958b3819cd9bade01e608bfa8c4f47433bdeea0971cf0fb6e8f1f1e21');assert.equal(createHash('sha256').update(readFileSync(p+m.patch)).digest('hex'),m.sha256);
});
