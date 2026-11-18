// PRE-P4-007 actual shipped xterm DECALN evidence, not native acceptance.
import {createRequire} from 'node:module';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {dirname,relative} from 'node:path';
const require=createRequire(import.meta.url),{Terminal}=require('@xterm/headless');
const output=process.argv[2];if(!output)throw Error('usage: node p4-007-oracle.mjs output');
const w=s=>({hex:Buffer.from(s).toString('hex')}),dec='\x1b#8';
const cases=[
 {name:'default-alignment-control',cols:6,rows:4,steps:[w('abc'),w(dec),w('q')]},
 {name:'bold-red-then-coherent-SGR1',cols:6,rows:4,steps:[w('\x1b[1;31m'),w(dec),w('\x1b[1m'),w('q')]},
 {name:'bold-red-then-coherent-SGR0',cols:6,rows:4,steps:[w('\x1b[1;31m'),w(dec),w('\x1b[0m'),w('q')]},
 {name:'basic-flags-RGB',cols:6,rows:4,steps:[w('\x1b[1;2;3;9;38;2;17;34;51;48;2;7;8;9m'),w(dec),w('\x1b[1;2;3;9m'),w('q')]},
 {name:'protected-E-current-already-coherent',cols:6,rows:4,steps:[w('\x1b[1"q'),w(dec),w('\x1b[0m'),w('q')]},
 {name:'margins-preserved-following-scroll',cols:6,rows:4,steps:[w('a\r\nb\r\nc\r\nd\x1b[2;3r'),w(dec),w('\x1b[3;1HR\nS')]},
 {name:'full-margins-origin-preserved',cols:6,rows:4,steps:[w('\x1b[?6h'),w(dec),w('\x1b[2;3r\x1b[1;1Hq')]},
 {name:'margins-and-origin-fill-start',cols:6,rows:4,steps:[w('a\r\nb\r\nc\r\nd\x1b[2;3r\x1b[?6h'),w(dec),w('q')]},
 {name:'history-base-saved-retained',cols:6,rows:4,steps:[w('0\r\n1\r\n2\r\n3\r\n4\r\n5\x1b[1;31m\x1b7'),w(dec),w('\x1b[1m'),w('\x1b8q')]},
 {name:'ALT-hidden-backing-styled',cols:6,rows:4,steps:[w('\x1b[?47hABCDEF\x1b[1;31m'),{resize:[4,4]},w(dec),w('\x1b[1m'),w('q')]},
 {name:'DECSTR-current-only-does-not-fix-old-E',cols:6,rows:4,steps:[w('\x1b[1;31m'),w(dec),w('\x1b[!p'),w('q')]},
 {name:'RIS-clears-body-control',cols:6,rows:4,steps:[w('\x1b[1;31m'),w(dec),w('\x1bc'),w('q')]},
];
function capture(t,b){return {x:b.x,y:b.y,base:b.ybase,scrollTop:b.scrollTop,scrollBottom:b.scrollBottom,savedX:b.savedX,savedBufferY:b.savedY,savedFG:b.savedCurAttrData.fg>>>0,savedBG:b.savedCurAttrData.bg>>>0,rows:Array.from({length:b.lines.length},(_,y)=>{const line=b.lines.get(y);return {wrapped:line.isWrapped,cells:Array.from({length:line.length},(_,x)=>{const c=line.loadCell(x,t.buffer.active.getNullCell());return {text:c.getChars(),width:c.getWidth(),rawFG:c.fg>>>0,rawBG:c.bg>>>0,protected:!!c.isProtected()};})};}),copy:Array.from({length:b.lines.length},(_,y)=>b.lines.get(y).translateToString(true)).join('\n').trimEnd()};}
let cuts=0;
for(const c of cases){const t=new Terminal({cols:c.cols,rows:c.rows,scrollback:8,allowProposedApi:true});for(const step of c.steps){if(step.hex!==undefined)await new Promise(done=>t.write(Buffer.from(step.hex,'hex'),done));else t.resize(...step.resize);const s=t._core._bufferService,p=t._core._inputHandler._curAttrData;step.expected={active:s.buffer===s.buffers.alt?'alternate':'primary',cols:t.cols,rows:t.rows,currentFG:p.fg>>>0,currentBG:p.bg>>>0,currentExtended:p.extended._ext>>>0,origin:t._core.coreService.decPrivateModes.origin,join:t._core._inputHandler._parser.precedingJoinState,primary:capture(t,s.buffers.normal),alternate:capture(t,s.buffers.alt)};cuts++;}t.dispose();}
const paths=[require.resolve('@xterm/headless'),'node_modules/@xterm/xterm/src/common/InputHandler.ts','node_modules/@xterm/xterm/src/common/buffer/BufferLine.ts','node_modules/@xterm/xterm/src/common/buffer/AttributeData.ts'];
const sourceHashes=paths.map(p=>({path:relative(process.cwd(),p).replaceAll('\\','/'),sha256:createHash('sha256').update(readFileSync(p)).digest('hex')}));
mkdirSync(dirname(output),{recursive:true});writeFileSync(output,JSON.stringify({status:'PRE-P4-007 actual source oracle, NOT native pass; unchanged006 negative tests pending',sourceHashes,summary:{cases:cases.length,cuts},cases},null,2)+'\n');console.log(JSON.stringify({cases:cases.length,cuts}));
