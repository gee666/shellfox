// Actual shipped headless xterm oracle. No browser/UI, production parser or model replacement.
import { createRequire } from 'node:module';
import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { relative } from 'node:path';
const require=createRequire(import.meta.url);const {Terminal}=require('@xterm/headless');
const w=text=>({hex:Buffer.from(text).toString('hex')});const resize=(cols,rows=4)=>({resize:[cols,rows]});
const operations=[
 ['CUP',[w('\x1b[2;2H')]],['HVP',[w('\x1b[3;2f')]],['CHA',[w('\x1b[2G')]],['HPA',[w('\x1b[2`')]],['VPA',[w('\x1b[3d')]],
 ['CUU',[w('\x1b[A')]],['CUD',[w('\x1b[B')]],['CUF',[w('\x1b[C')]],['CUB',[w('\x1b[D')]],['CNL',[w('\x1b[E')]],['CPL',[w('\x1b[F')]],['HPR',[w('\x1b[2a')]],['VPR',[w('\x1b[2e')]],
 ['HT',[w('\t')]],['CHT',[w('\x1b[I')]],['CBT',[w('\x1b[Z')]],['HTS',[w('\x1bH')]],['TBC',[w('\x1b[g')]],
 ['CR',[w('\r')]],['LF',[w('\n')]],['VT',[w('\v')]],['FF',[w('\f')]],['BS',[w('\b')]],['BS-reverse',[w('\x1b[?45h'),w('\b')]],
 ['IND',[w('\x1bD')]],['NEL',[w('\x1bE')]],['RI',[w('\x1bM')]],
 ['SC',[w('\x1b7')]],['RC',[w('\x1b7'),w('\x1b[2;2H'),w('\x1b8')]],['ANSI-SC-RC',[w('\x1b[s'),w('\x1b[2;2H'),w('\x1b[u')]],
 ['save-1048',[w('\x1b[?1048h')]],['restore-1048',[w('\x1b[?1048h'),w('\x1b[2;2H'),w('\x1b[?1048l')]],
 ['RIS',[w('\x1bc')]],['DECSTR',[w('\x1b[!p')]],
 ['resize-grow',[resize(6)]],['resize-shrink',[resize(3)]],['resize-rows',[resize(4,5)]],['resize-noop',[resize(4)]],
 ['alt-47',[w('\x1b[?47h')]],['normal-47',[w('\x1b[?47l')]],['alt-47-roundtrip',[w('\x1b[?47h'),w('\x1b[?47l')]],
 ['alt-1047',[w('\x1b[?1047h')]],['normal-1047',[w('\x1b[?1047l')]],['alt-1049',[w('\x1b[?1049h')]],['normal-1049',[w('\x1b[?1049l')]],['alt-1049-roundtrip',[w('\x1b[?1049h'),w('\x1b[?1049l')]],
 ['origin-set',[w('\x1b[?6h')]],['origin-reset',[w('\x1b[?6l')]],['margins',[w('\x1b[2;4r')]],['margins-reset',[w('\x1b[r')]],
 ['SGR-no-motion',[w('\x1b[32m')]],['OSC-no-motion',[w('\x1b]0;cursor\x1b\\')]],['wrap-reset-mode',[w('\x1b[?7l')]],
 ['ICH',[w('\x1b[@')]],['DCH',[w('\x1b[P')]],['ECH',[w('\x1b[X')]],['EL',[w('\x1b[K')]],['ED',[w('\x1b[J')]],['IL',[w('\x1b[L')]],['DL',[w('\x1b[M')]],['SU',[w('\x1b[S')]],['SD',[w('\x1b[T')]],
];
const seeds=[['ordinary','A'],['pending','ABCD'],['overflow','ABCD\x1b[31m\u0301'],['saved-pending','ABCD\x1b7'],['saved-overflow','ABCD\x1b[31m\u0301\x1b7']];
const cases=[];
for(const alternate of [false,true])for(const [seed,text]of seeds)for(const [transition,steps]of operations){
 const setup=[...(alternate?[w('\x1b[?1049h')]:[]),w('\x1b[2;1H'),w(text)];
 cases.push({name:`${alternate?'alt':'primary'}-${seed}-${transition}`,transition,seed,alternate,cols:4,rows:4,setupCount:setup.length,operationCount:steps.length,steps:[...setup,...steps,w('X')].map(step=>({...step}))});
}
function capture(t){const bs=t._core._bufferService, b=bs.buffer;return {x:b.x,y:b.y,base:b.ybase,savedX:b.savedX,savedY:b.savedY,alternate:b===bs.buffers.alt,join:t._core._inputHandler._parser.precedingJoinState,copy:Array.from({length:b.lines.length},(_,y)=>b.lines.get(y).translateToString(true)).join('\n').trimEnd(),rows:Array.from({length:t.rows},(_,y)=>{const line=t.buffer.active.getLine(b.ybase+y);return Array.from({length:line.length},(_,x)=>{const cell=line.getCell(x);return {text:cell.getChars(),width:cell.getWidth()};});})};}
for(const c of cases){const t=new Terminal({cols:c.cols,rows:c.rows,allowProposedApi:true});if(t._core.unicodeService.activeVersion!=='6')throw Error('Unicode provider changed');for(const step of c.steps){if(step.hex!==undefined)await new Promise(done=>t.write(Buffer.from(step.hex,'hex'),done));else t.resize(...step.resize);step.expected=capture(t);}t.dispose();}
const sources=[require.resolve('@xterm/headless'),'node_modules/@xterm/headless/package.json','node_modules/@xterm/xterm/src/common/InputHandler.ts','node_modules/@xterm/xterm/src/common/buffer/Buffer.ts','node_modules/@xterm/xterm/src/common/buffer/BufferSet.ts','node_modules/@xterm/xterm/src/common/CoreTerminal.ts'].map(path=>({path:relative(process.cwd(),path).replaceAll('\\','/'),sha256:createHash('sha256').update(readFileSync(path)).digest('hex')}));
const output=process.argv[2];if(!output)throw Error('usage: node cursor-transitions.mjs tmp/.../oracle.json');writeFileSync(output,JSON.stringify({status:'pre-edit-shipped-source-oracle',sources,cases},null,2)+'\n');console.log(JSON.stringify({cases:cases.length,cuts:cases.reduce((n,c)=>n+c.steps.length,0),transitions:operations.length,output}));
