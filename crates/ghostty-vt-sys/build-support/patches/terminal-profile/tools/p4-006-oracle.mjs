// Actual shipped xterm DECSTR/OSC8 source evidence. No replacement parser.
import {createRequire} from 'node:module';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {dirname,relative} from 'node:path';
const require=createRequire(import.meta.url),{Terminal}=require('@xterm/headless');
const output=process.argv[2];if(!output)throw Error('usage: node p4-006-oracle.mjs output');
const open='\x1b]8;;https://example.test/A\x1b\\',other='\x1b]8;id=B;https://example.test/B\x07',close='\x1b]8;;\x1b\\',decstr='\x1b[!p',ris='\x1bc';
const w=(s,admission='success')=>({hex:Buffer.from(s).toString('hex'),admission});
const cases=[
 {name:'no-output-open-implicit-ST',steps:[w(open,'Unsupported'),w(decstr),w('B')]},
 {name:'no-output-open-explicit-BEL',steps:[w(other,'Unsupported'),w(decstr),w('B')]},
 {name:'no-output-reopen',steps:[w(open,'Unsupported'),w(other,'Unsupported'),w(decstr),w('B')]},
 {name:'SGR0-does-not-close-pending-URI',steps:[w(open,'Unsupported'),w('\x1b[0m','Unsupported'),w(decstr),w('B')]},
 {name:'explicit-close-before-DECSTR',steps:[w(open,'Unsupported'),w(close),w(decstr),w('B')]},
 {name:'DECSTR-then-close-and-repeat',steps:[w(open,'Unsupported'),w(decstr),w(close+decstr),w('B')]},
 {name:'historical-cell-A-pending-B',steps:[w(open+'H','StateUnknown'),w(other,'StateUnknown'),w(decstr,'StateUnknown'),w('U','StateUnknown')]},
 {name:'historical-cell-closed-before-reset',steps:[w(open+'H'+close,'StateUnknown'),w(decstr,'StateUnknown'),w('U','StateUnknown')]},
 {name:'save-restore-after-reset',steps:[w(open,'Unsupported'),w(decstr),w('\x1b7'),w('\x1b8B')]},
 {name:'active-ALT-reset',steps:[w('\x1b[?47h'),w(open,'Unsupported'),w(decstr),w('B')]},
 {name:'source-global-pending-URI-DECSTR-in-ALT',steps:[w(open,'Unsupported'),w('\x1b[?47h','Unsupported'),w(decstr),w('A'),w('\x1b[?47l'),w('B')]},
 {name:'no-output-open-RIS',steps:[w(open,'Unsupported'),w(ris),w('B')]},
 {name:'printed-link-RIS',steps:[w(open+'H','StateUnknown'),w(ris),w('B')]},
 {name:'SGR0-pending-print-then-DECSTR',steps:[w(open,'Unsupported'),w('\x1b[0m','Unsupported'),w('H','StateUnknown'),w(decstr,'StateUnknown'),w('U','StateUnknown')]},
 {name:'no-URI-reset-idempotent',steps:[w(decstr),w(decstr),w(ris),w('B')]},
];
function uri(t,id){return id?t._core._oscLinkService.getLinkData(id)?.uri??'':'';}
function buffer(t,b){return {x:b.x,y:b.y,base:b.ybase,savedX:b.savedX,savedBufferY:b.savedY,savedFG:b.savedCurAttrData.fg>>>0,savedBG:b.savedCurAttrData.bg>>>0,rows:Array.from({length:b.lines.length},(_,y)=>{const line=b.lines.get(y);return Array.from({length:line.length},(_,x)=>{const c=line.loadCell(x,t.buffer.active.getNullCell());return {text:c.getChars(),width:c.getWidth(),uri:uri(t,c.hasExtendedAttrs()?c.extended.urlId:0)};});}),copy:Array.from({length:b.lines.length},(_,y)=>b.lines.get(y).translateToString(true)).join('\n').trimEnd()};}
let cuts=0,admitted=0;
for(const c of cases){const t=new Terminal({cols:12,rows:4,scrollback:8,allowProposedApi:true});for(const step of c.steps){await new Promise(done=>t.write(Buffer.from(step.hex,'hex'),done));const service=t._core._bufferService,p=t._core._inputHandler._curAttrData;step.expected={active:service.buffer===service.buffers.alt?'alternate':'primary',currentFG:p.fg>>>0,currentBG:p.bg>>>0,currentExtended:p.extended._ext>>>0,currentUri:uri(t,p.extended.urlId),join:t._core._inputHandler._parser.precedingJoinState,primary:buffer(t,service.buffers.normal),alternate:buffer(t,service.buffers.alt)};cuts++;if(step.admission==='success')admitted++;}t.dispose();}
const paths=[require.resolve('@xterm/headless'),'node_modules/@xterm/xterm/src/common/InputHandler.ts','node_modules/@xterm/xterm/src/common/buffer/AttributeData.ts','node_modules/@xterm/xterm/src/common/buffer/BufferSet.ts'];
const sourceHashes=paths.map(p=>({path:relative(process.cwd(),p).replaceAll('\\','/'),sha256:createHash('sha256').update(readFileSync(p)).digest('hex')}));
mkdirSync(dirname(output),{recursive:true});writeFileSync(output,JSON.stringify({status:'PRIVATE006 source oracle, not native acceptance; admission expectations are documented first-subset policy',sourceHashes,summary:{cases:cases.length,cuts,admitted,refused:cuts-admitted},cases},null,2)+'\n');console.log(JSON.stringify({cases:cases.length,cuts,admitted,refused:cuts-admitted}));
