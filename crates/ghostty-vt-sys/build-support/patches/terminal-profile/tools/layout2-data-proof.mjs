// PUBLIC C only. Reads pinned DOM evidence; never launches a GUI/browser/parser.
import { readFileSync, writeFileSync, mkdirSync, copyFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import assert from 'node:assert/strict';
import { fixtures } from './dom-render-contract.mjs'; // fixture definitions only; no collectDomEvidence call
const [installArg,seedsArg,mode='experimental']=process.argv.slice(2);
if(!installArg||!seedsArg)throw Error('usage: node layout2-data-proof.mjs private-install private-seeds [experimental|off]');
const root=process.cwd(),patch='crates/ghostty-vt-sys/build-support/patches/terminal-profile';
const out=resolve(root,'tmp/ghostty-profile/layout2-data-proof',mode);mkdirSync(out,{recursive:true});
const install=resolve(installArg),seeds=resolve(seedsArg);
const evidence=JSON.parse(readFileSync(`${patch}/prototype/render-contract/evidence.json`,'utf8'));
let header='';const names=[];
for(const [i,c]of fixtures.entries()){
 const steps=c.synthetic?[]:c.steps;const code=[];
 for(const [j,s]of steps.entries()){
  if(s.text!==undefined){const bytes=Buffer.from(s.text);header+=`static const uint8_t input_${i}_${j}[]={${[...bytes].join(',')}};\n`;code.push(`{input_${i}_${j},sizeof(input_${i}_${j}),0}`);}
  else code.push(`{NULL,0,${s.resize}}`);
 }
 header+=`static const Step steps_${i}[]={${code.length?code.join(','):'{NULL,0,0}'}};\n`;
 const seed=c.synthetic==='zero-stub'?1:c.synthetic?2:0;
 names.push(`{${JSON.stringify(c.name)},${seed},${c.cols},steps_${i},${steps.length},${c.cursorX??-1}}`);
}
header+=`static const Case cases[]={${names.join(',\n')}};\n`;writeFileSync(`${out}/layout2-data-cases.h`,header);
function run(command,args){const result=spawnSync(command,args,{encoding:'utf8',maxBuffer:16*1024*1024,env:process.env});if(result.status!==0){writeFileSync(`${out}/failure.stdout`,result.stdout||'');writeFileSync(`${out}/failure.stderr`,result.stderr||'');throw Error(`${command} failed ${result.status}; evidence under ${out}\n${result.stderr}`);}return result;}
const exe=`${out}/layout2-data-api.exe`;
run(resolve('tmp/toolchains/zig-0.17.0/zig-x86_64-windows-0.17.0/zig.exe'),['cc','-std=c11','-Wall','-Wextra','-Werror','-target','x86_64-windows-gnu','-I',`${install}/include`,'-I',out,`${patch}/tests/layout2-data-api.c`,'-L',`${install}/lib`,'-lghostty-vt','-o',exe]);
copyFileSync(`${install}/bin/ghostty-vt.dll`,`${out}/ghostty-vt.dll`);
const result=run(exe,[mode,'6'.repeat(64),seeds]);writeFileSync(`${out}/public-c.jsonl`,result.stdout);writeFileSync(`${out}/public-c.stderr`,result.stderr);
if(mode==='off'){assert.match(result.stdout,/defaultOFF/);console.log(result.stdout.trim());process.exit(0);}
const frames=result.stdout.trim().split(/\r?\n/).map(line=>JSON.parse(line));assert.equal(frames.length,(fixtures.length+2)*2);
const hex=s=>Buffer.from(s).toString('hex');
const near=(a,b)=>assert.ok(Math.abs(a-b)<0.06,`flow ${a} versus pinned DOM ${b}`);
for(let i=0;i<fixtures.length+2;i++){
 const a=frames[i*2],b=frames[i*2+1];assert.equal(a.case,i);assert.equal(b.pass,1);assert.deepEqual({...a,pass:1},b,`public metadata source->format2->consumer case ${i}`);
 if(i===fixtures.length){assert.deepEqual(a.cursor,[3,0,1,1,5]);assert.deepEqual(a.saved,[1,3,0,1,1]);continue;}
 if(i===fixtures.length+1){const c=a.data[0].cells[0];assert.equal(c.text,'41cc81');assert.equal(c.uri,hex('https://example.invalid/managed'));assert.deepEqual(c.bg,[2,0x5a646e]);assert.ok(c.style[0]&1);assert.ok(c.style[0]&2);continue;}
 const expected=evidence.cases.find(c=>c.name===fixtures[i].name);assert.ok(expected);
 assert.equal(a.cols,expected.cols);assert.deepEqual([a.cursor[4],a.cursor[1]],expected.cursor);
 const row=a.data[0];assert.equal(row.backing,expected.backingCols);assert.equal(row.trim,expected.noBgTrimmedLength);
 assert.equal(row.cells.length,expected.cells.length);
 for(const [x,c]of row.cells.entries()){
  const golden=expected.cells[x];assert.equal(c.text,hex(golden.text),`${expected.name} source${x} text`);assert.equal(c.semantic,golden.semantic,`${expected.name} source${x} semantic`);assert.equal(c.stride,1);
  const mode=(golden.bg>>>24)&3;assert.equal(c.bg[0],mode===0?0:mode===3?2:1);assert.equal(c.bg[1],mode===0?0:golden.bg&0xffffff);
  const link=expected.links.find(l=>l.range.start.y===1&&l.range.end.y===1&&x+1>=l.range.start.x&&x+1<=l.range.end.x);
  assert.equal(c.uri,link?hex(link.text):'');assert.equal(c.has_link,link?1:0);
 }
 const emitted=row.cells.filter(c=>c.emit);let index=0;
 assert.equal(emitted.map(c=>Buffer.from(c.text,'hex').toString()||' ').join(''),expected.spans.map(s=>s.text).join(''));
 for(const span of expected.spans){const start=emitted[index];assert.ok(start);near(start.flow,span.left);let text='';while(index<emitted.length&&text.length<span.text.length){const c=emitted[index++];text+=Buffer.from(c.text,'hex').toString()||' ';}assert.equal(text,span.text);}
 assert.equal(index,emitted.length);
 const classSources=row.cells.filter(c=>c.cursor_class).map(c=>c.x);const classSpans=expected.spans.filter(s=>s.classes.includes('xterm-cursor'));
 assert.equal(classSources.length,classSpans.length,expected.name+' cursor skip');
 if(classSources.length)assert.equal(classSources[0],a.cursor[0]);
}
assert.match(result.stderr,/kind-mask=ff/);
console.log(JSON.stringify({publicC:'passed',fixtures:fixtures.length,virtualSavedCursorCases:1,managedGraphemeStyleLinkCases:1,frames:frames.length,kinds:8,sourceUnchanged:'byte-identical reencode before/after reads',output:out}));
