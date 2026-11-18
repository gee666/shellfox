// PRE008 actual shipped xterm SPA/EPA/protection oracle; no producer acceptance.
import {createRequire} from 'node:module';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {dirname,relative} from 'node:path';
const require=createRequire(import.meta.url),{Terminal}=require('@xterm/headless');
const output=process.argv[2];if(!output)throw Error('usage: node p4-008-oracle.mjs output');
const w=s=>({hex:Buffer.from(s).toString('hex')}),A='\x1b[1"qA',spa='\x1bV',epa='\x1bW',dec='\x1b[1"q',reset='\x1b[!p',el='\x1b[2K',ed='\x1b[2J',alt='\x1b[?47h',normal='\x1b[?47l';
const cases=[
 {name:'DEC-protectedA-ordinaryEL-control',steps:[w(A),w(el),w('q')]},
 {name:'primary-ISO-EL-DECSTR-persistentA',steps:[w(A),w(spa),w(el),w(reset),w('q')]},
 {name:'primary-ISO-ED-DECSTR-persistentA',steps:[w(A),w(spa),w(ed),w(reset),w('q')]},
 {name:'EPA-does-not-reset-native-mode',steps:[w(A),w(spa),w(epa),w(el),w(reset),w('q')]},
 {name:'DEC-mode-before-DECSTR-keeps-history',steps:[w(A),w(spa),w(el),w(dec),w(reset),w('q')]},
 {name:'dormant-primary-ALTreset-return-erase-DEC',steps:[w(A),w(spa),w(alt),w(reset),w(normal),w(el),w(dec),w('q')]},
 {name:'dormant-primary-ISO-after-ALT-DECSTR',steps:[w(A),w(spa),w(alt),w(reset)]},
 {name:'print-during-ISO-existing-latch-control',steps:[w(spa),w('B'),w(el),w(reset),w('q')]},
 {name:'no-output-ISO-DECSTR-control',steps:[w(spa),w(reset),w('q')]},
 {name:'actual-RIS-clears-primary-bad-body',steps:[w(A),w(spa),w(el),w(reset),w('\x1bc'),w('q')]},
 {name:'ALT-hidden-ISO-specialEL-qualification',steps:[w(alt),{resize:[4,4]},w(A),w(spa),w(el),w(reset),w('q')]},
 {name:'ALT-hidden-ISO-ED-qualification',steps:[w(alt),{resize:[4,4]},w(A),w(spa),w(ed),w(reset),w('q')]},
];
function buffer(t,b){return {x:b.x,y:b.y,base:b.ybase,scrollTop:b.scrollTop,scrollBottom:b.scrollBottom,savedX:b.savedX,savedBufferY:b.savedY,savedFG:b.savedCurAttrData.fg>>>0,savedBG:b.savedCurAttrData.bg>>>0,rows:Array.from({length:b.lines.length},(_,y)=>{const line=b.lines.get(y);return {wrapped:line.isWrapped,cells:Array.from({length:line.length},(_,x)=>{const c=line.loadCell(x,t.buffer.active.getNullCell());return {text:c.getChars(),width:c.getWidth(),rawFG:c.fg>>>0,rawBG:c.bg>>>0,protected:!!c.isProtected()};})};}),copy:Array.from({length:b.lines.length},(_,y)=>b.lines.get(y).translateToString(true)).join('\n').trimEnd()};}
let cuts=0;for(const c of cases){c.cols=6;c.rows=4;const t=new Terminal({cols:6,rows:4,scrollback:8,allowProposedApi:true});for(const step of c.steps){if(step.hex!==undefined)await new Promise(done=>t.write(Buffer.from(step.hex,'hex'),done));else t.resize(...step.resize);const s=t._core._bufferService,p=t._core._inputHandler._curAttrData;step.expected={active:s.buffer===s.buffers.alt?'alternate':'primary',cols:t.cols,rows:t.rows,currentFG:p.fg>>>0,currentBG:p.bg>>>0,currentExtended:p.extended._ext>>>0,origin:t._core.coreService.decPrivateModes.origin,join:t._core._inputHandler._parser.precedingJoinState,primary:buffer(t,s.buffers.normal),alternate:buffer(t,s.buffers.alt)};cuts++;}t.dispose();}
const paths=[require.resolve('@xterm/headless'),'node_modules/@xterm/xterm/src/common/InputHandler.ts','node_modules/@xterm/xterm/src/common/buffer/BufferLine.ts','node_modules/@xterm/xterm/src/common/buffer/AttributeData.ts'];const sourceHashes=paths.map(p=>({path:relative(process.cwd(),p).replaceAll('\\','/'),sha256:createHash('sha256').update(readFileSync(p)).digest('hex')}));
mkdirSync(dirname(output),{recursive:true});writeFileSync(output,JSON.stringify({status:'PRE008 actual source oracle; SPA/EPA fallback and persistent ISO hypothesis await unchanged007 negative fixture; NOT producer pass',sourceHashes,summary:{cases:cases.length,cuts},cases},null,2)+'\n');console.log(JSON.stringify({cases:cases.length,cuts}));
