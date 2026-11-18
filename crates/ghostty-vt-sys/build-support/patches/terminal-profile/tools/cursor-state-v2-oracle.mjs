// PRE-EDIT state/ABI planning evidence from actual shipped xterm, not a replacement parser.
import {createRequire} from 'node:module';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {dirname,relative} from 'node:path';
import {createHash} from 'node:crypto';
const require=createRequire(import.meta.url),{Terminal}=require('@xterm/headless');
const w=text=>({hex:Buffer.from(text).toString('hex')}),resize=(cols,rows)=>({resize:[cols,rows]});
const cases=[
 {name:'saved-before-history-growth',cols:6,rows:3,scrollback:8,steps:[w('\x1b[2;1HA\x1b7'),w('\x1b[3;1HB\r\nC\r\nD'),w('\x1b8'),w('X')]},
 {name:'saved-at-nonzero-base',cols:6,rows:3,scrollback:8,steps:[w('0\r\n1\r\n2\r\n3'),w('\x1b[2;2H\x1b7'),w('\x1b[3;1H4\r\n5\r\n6'),w('\x1b8'),w('X')]},
 {name:'ordinary-capacity-recycle-keeps-numeric-savedY',cols:6,rows:3,scrollback:2,steps:[w('0\r\n1\r\n2\r\n3\r\n4\x1b7'),w('\r\n5\r\n6\r\n7\r\n8'),w('\x1b8'),w('X')]},
 {name:'scrollback-option-trim-adjusts-savedY',cols:6,rows:3,scrollback:8,steps:[w('0\r\n1\r\n2\r\n3\r\n4\x1b7'),{scrollback:0},w('\x1b8'),w('X')]},
 {name:'ED3-trim-does-not-adjust-savedY',cols:6,rows:3,scrollback:8,steps:[w('0\r\n1\r\n2\r\n3\r\n4\x1b7'),w('\x1b[3J'),w('\x1b8'),w('X')]},
 {name:'row-growth-pulls-history-without-rebuilding-savedY',cols:6,rows:3,scrollback:8,steps:[w('0\r\n1\r\n2\r\n3\r\n4'),w('\x1b[2;2H\x1b7\x1b[3;1H'),resize(6,5),w('\x1b8'),w('X')]},
 {name:'save-before-growth-then-resize-must-not-rebase-old-relativeY',cols:6,rows:3,scrollback:8,steps:[w('\x1b[2;1HA\x1b7'),w('\x1b[3;1HB\r\nC\r\nD'),resize(6,5),w('\x1b8'),w('X')]},
 {name:'saved-before-reflowed-group',cols:8,rows:4,scrollback:8,steps:[w('A\x1b7\r\nBCDEFGHIJKLM\r\nCUR'),resize(4,4),w('\x1b8'),w('X'),resize(8,4)]},
 {name:'saved-nonzero-base-reflow-both-directions',cols:8,rows:3,scrollback:8,steps:[w('ABCDEFGHJKLM\r\nROW\r\nNEXT\r\nCUR'),w('\x1b[2;2H\x1b7\x1b[3;1H'),resize(4,3),resize(10,3),w('\x1b8'),w('X')]},
 {name:'no-explicit-save-still-has-source-saved-defaults',cols:8,rows:4,scrollback:8,steps:[w('A\r\nBCDEFGHIJKLM\r\nCUR'),resize(4,4),w('\x1b8'),w('X')]},
 {name:'DECSTR-savedY-is-current-base',cols:6,rows:3,scrollback:8,steps:[w('0\r\n1\r\n2\r\n3'),w('\x1b[!p'),w('\r\n4\r\n5'),w('\x1b8'),w('X')]},
 {name:'ALT-clear-keeps-saved-coordinates-unlike-new-buffer',cols:6,rows:3,scrollback:8,steps:[w('\x1b[?47h\x1b[2;3H\x1b7'),w('\x1b[?47l'),w('\x1b[?47h\x1b8'),w('X'),w('\x1bc')]},
 {name:'unfilled-ALT-savedX-survives-shrink',cols:4,rows:3,scrollback:8,steps:[w('\x1b[?47hABCD\x1b[31m\u0301\x1b7'),w('\x1b[?47l'),resize(3,3),w('\x1b[?47h\x1b8'),w('X')]},
 {name:'reverse-BS-DECAWMoff-pending',cols:4,rows:3,scrollback:8,steps:[w('ABCD'),w('\x1b[?45h\x1b[?7l'),w('\b'),w('X')]},
 {name:'reverse-BS-DECAWMoff-overflow',cols:4,rows:3,scrollback:8,steps:[w('ABCD\x1b[31m\u0301'),w('\x1b[?45h\x1b[?7l'),w('\b'),w('X')]},
 {name:'reverse-BS-actual-wide-earlywrap-leftedge',cols:4,rows:3,scrollback:8,steps:[w('ABC中'),w('\x1b[2;1H\x1b[?45h'),w('\b'),w('X')]},
 {name:'reverse-BS-DECAWMoff-wide-earlywrap-leftedge',cols:4,rows:3,scrollback:8,steps:[w('ABC中'),w('\x1b[2;1H\x1b[?45h\x1b[?7l'),w('\b'),w('X')]},
 {name:'HTS-at-pending-survives-growth',cols:4,rows:3,scrollback:8,steps:[w('\x1b[3gABCD'),w('\x1bH'),resize(6,3),w('\r\t'),w('X')]},
 {name:'HTS-at-overflow-survives-growth',cols:4,rows:3,scrollback:8,steps:[w('\x1b[3gABCD\x1b[31m\u0301'),w('\x1bH'),resize(8,3),w('\r\t'),w('X')]},
 {name:'TBC-at-physical3-must-not-remove-virtual4-stop',cols:4,rows:3,scrollback:8,steps:[w('\x1b[3gABCD\x1bH'),w('\x1b[1;4H\x1b[g'),resize(6,3),w('\r\t'),w('X')]},
 {name:'TBC-at-pending-must-not-remove-physical3-stop',cols:4,rows:3,scrollback:8,steps:[w('\x1b[3g\x1b[1;4H\x1bHD'),w('\x1b[g'),resize(6,3),w('\r\t'),w('X')]},
 {name:'TBC-at-overflow-must-not-remove-physical3-stop',cols:4,rows:3,scrollback:8,steps:[w('\x1b[3g\x1b[1;4H\x1bHD\x1b[31m\u0301'),w('\x1b[g'),resize(8,3),w('\r\t'),w('X')]},
 {name:'off-grid-tab-stop-retained-on-shrink',cols:12,rows:3,scrollback:8,steps:[w('\x1b[3g\x1b[1;11H\x1bH'),resize(4,3),resize(12,3),w('\r\t'),w('X')]},
 {name:'tab-tables-are-per-buffer',cols:8,rows:3,scrollback:8,steps:[w('\x1b[3g\x1b[1;3H\x1bH'),w('\x1b[?47h\x1b[3g\x1b[1;6H\x1bH'),w('\x1b[?47l\r\t'),w('X'),w('\x1b[?47h\r\t')]},
 {name:'unfilled-ALT-tab-table-also-resizes',cols:8,rows:3,scrollback:8,steps:[w('\x1b[3g\x1b[1;3H\x1bH'),resize(12,3),w('\x1b[?47h\r\t'),w('X'),w('\x1b[?47l\r\t')]},
];
function captureBuffer(t,b){
 const rows=Array.from({length:b.lines.length},(_,y)=>{const line=b.lines.get(y);return {wrapped:line.isWrapped,cells:Array.from({length:line.length},(_,x)=>{const cell=line.loadCell(x,t.buffer.active.getNullCell());return {text:cell.getChars(),width:cell.getWidth()};})};});
 return {x:b.x,y:b.y,base:b.ybase,display:b.ydisp,savedX:b.savedX,savedBufferY:b.savedY,scrollTop:b.scrollTop,scrollBottom:b.scrollBottom,maxLines:b.lines.maxLength,tabs:Object.keys(b.tabs).filter(k=>b.tabs[k]).map(Number).sort((a,b)=>a-b),rows,copy:Array.from({length:b.lines.length},(_,y)=>b.lines.get(y).translateToString(true)).join('\n').trimEnd()};
}
let nonzeroBaseCuts=0,outOfExtentSavedCuts=0,offGridTabCuts=0;
for(const c of cases){const t=new Terminal({cols:c.cols,rows:c.rows,scrollback:c.scrollback,allowProposedApi:true});if(t._core.unicodeService.activeVersion!=='6')throw Error('Unexpected Unicode provider');for(const step of c.steps){if(step.hex!==undefined)await new Promise(done=>t.write(Buffer.from(step.hex,'hex'),done));else if(step.resize)t.resize(...step.resize);else t.options.scrollback=step.scrollback;
 const service=t._core._bufferService,modes=t._core.coreService.decPrivateModes;
 const primary=captureBuffer(t,service.buffers.normal),alternate=captureBuffer(t,service.buffers.alt);
 step.expected={cols:t.cols,rows:t.rows,active:service.buffer===service.buffers.alt?'alternate':'primary',wraparound:modes.wraparound,reverseWraparound:modes.reverseWraparound,join:t._core._inputHandler._parser.precedingJoinState,primary,alternate};
 for(const b of [primary,alternate]){if(b.base!==0)nonzeroBaseCuts++;if(b.rows.length>0&&b.savedBufferY>=b.rows.length)outOfExtentSavedCuts++;if(b.tabs.some(x=>x>=t.cols))offGridTabCuts++;}
 }t.dispose();}
const sourcePaths=[require.resolve('@xterm/headless'),'node_modules/@xterm/headless/package.json','node_modules/@xterm/xterm/src/common/InputHandler.ts','node_modules/@xterm/xterm/src/common/buffer/Buffer.ts','node_modules/@xterm/xterm/src/common/buffer/BufferSet.ts','node_modules/@xterm/xterm/src/common/services/BufferService.ts'];
const sourceHashes=sourcePaths.map(p=>({path:relative(process.cwd(),p).replaceAll('\\','/'),sha256:createHash('sha256').update(readFileSync(p)).digest('hex')}));
const output=process.argv[2];if(!output)throw Error('usage: node cursor-state-v2-oracle.mjs tmp/.../oracle.json');mkdirSync(dirname(output),{recursive:true});const summary={cases:cases.length,cuts:cases.reduce((n,c)=>n+c.steps.length,0),nonzeroBaseBufferCuts:nonzeroBaseCuts,savedOutsideCurrentRowsBufferCuts:outOfExtentSavedCuts,offGridTabBufferCuts:offGridTabCuts};writeFileSync(output,JSON.stringify({status:'pre-P4-004-state-and-ABI-plan-oracle-not-producer-pass',sourceHashes,summary,cases},null,2)+'\n');console.log(JSON.stringify(summary));
