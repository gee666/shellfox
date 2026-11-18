// Golden cells/copy/cursor/parser-state from shipped xterm6, not a terminal model.
import { createRequire } from 'node:module';
import { writeFileSync } from 'node:fs';
const require=createRequire(import.meta.url);
const {Terminal}=require('@xterm/headless');
const destination=process.argv[2];if(!destination) throw new Error('usage: node layout2-cases.mjs output.json');
const write=s=>({hex:Buffer.from(s).toString('hex')});
const raw=hex=>({hex});
const resize=(cols,rows)=>({resize:[cols,rows]});
const cases=[
 {name:'leading-combiner',cols:8,rows:3,steps:[write('\u0301'),write('A')]},
 {name:'sgr-reset-combiner',cols:8,rows:3,steps:[write('A'),write('\x1b[31m'),write('\u0301'),write('B')]},
 {name:'nonemoji-vs16',cols:8,rows:3,steps:[write('A'),write('\ufe0f'),write('B')]},
 {name:'leading-zero-sequence',cols:8,rows:3,steps:[write('\u0301\u200d\ufe0f'),write('A')]},
 {name:'unicode-base-joins',cols:8,rows:3,steps:[write('A\u0301\u200d\ufe0f'),write('B😀中')]},
 {name:'partial-csi',cols:8,rows:3,steps:[write('A'),write('\x1b'),write('['),write('3'),write('1'),write('m'),write('B\u0301')]},
 {name:'partial-utf8',cols:8,rows:3,steps:[write('A'),raw('f0'),raw('9f'),raw('98'),raw('80'),write('\u0301')]},
 {name:'partial-osc',cols:8,rows:3,steps:[write('A'),write('\x1b]0;ti'),write('tle'),write('\x1b'),write('\\'),write('B\u0301')]},
 {name:'nul-csi-boundary',cols:8,rows:3,steps:[write('A'),write('\x1b[3'),raw('00'),write('1m'),write('B\u0301')]},
 {name:'rep-boundaries',cols:8,rows:3,steps:[write('A\u0301'),write('\x1b[2'),write('b'),write('\x1b[b'),write('B'),write('\x1b[31m'),write('\x1b[2b'),write('C')]},
 {name:'insert-delete-erase',cols:8,rows:3,steps:[write('ABCD'),write('\x1b[3D'),write('\u0301'),write('\x1b[4h'),write('Q'),write('\x1b[4l'),write('\x1b[2D'),write('\x1b[P'),write('\x1b[X'),write('\x1b[2K')]},
 {name:'both-screens',cols:8,rows:3,steps:[write('\u0301A'),write('\x1b[?1049h'),write('\u0301B'),write('\x1b[?1049l'),write('C')]},
 {name:'reflow-leading-zero',cols:8,rows:3,steps:[write('\u0301ABCDE'),resize(4,3),resize(8,3)]},
 {name:'wide-and-zero',cols:8,rows:3,steps:[write('中\u0301'),write('\x1b[31m'),write('\u0301B')]},
 {name:'reflow-wrapped-cursor-zero',cols:6,rows:4,steps:[write('\u0301ABCDEFGHIJKLM'),resize(4,4),resize(8,4),write('Z\u0301'),write('\x1b[b')]},
 {name:'reflow-other-history-zero',cols:6,rows:3,steps:[write('\u0301ABCDEFGHIJKLMN\r\nCUR'),resize(4,3),resize(8,3),write('Z\u0301')]},
 {name:'reflow-other-history-wide',cols:8,rows:4,steps:[write('\u0301中A\u0301BCDEFGHIJ\r\nCUR\u0301'),resize(5,4),resize(9,4),write('Z')]},
 {name:'saved-cursor-shrink-grow',cols:8,rows:4,steps:[write('\u0301ABCDEF'),write('\x1b7'),write('\r\nOTHER'),resize(4,4),resize(8,4),write('\x1b8'),write('S\u0301')]},
 {name:'saved-cursor-shrink-suffix',cols:8,rows:4,steps:[write('\u0301ABCDEF'),write('\x1b7'),write('\r\nOTHER'),resize(4,4),write('\x1b8'),write('S\u0301')]},
 {name:'both-screens-resize-suffix',cols:8,rows:4,steps:[write('\u0301中ABCDE'),write('\x1b[?1049h'),write('\u0301ABCDEF'),resize(4,4),write('Z\u0301'),resize(8,4),write('\x1b[?1049l'),write('P\u0301')]},
 {name:'pending-cursor-resize',cols:6,rows:3,steps:[write('\u0301ABCDE'),resize(4,3),resize(8,3),write('Z')]},
 {name:'pending-zero-overflow',cols:4,rows:3,steps:[write('ABCD'),write('\x1b[31m'),write('\u0301'),write('B')]},
 {name:'pending-zero-double',cols:4,rows:3,steps:[write('ABCD'),write('\x1b[31m'),write('\u0301'),write('\ufe0f'),write('X')]},
 {name:'pending-zero-grow',cols:4,rows:3,steps:[write('ABCD'),write('\x1b[31m'),write('\u0301'),resize(8,3),write('Z')]},
 {name:'pending-zero-no-wrap',cols:4,rows:3,steps:[write('\x1b[?7l'),write('ABCD'),write('\x1b[31m'),write('\u0301'),write('\ufe0f'),write('X')]},
];
for(const entry of cases) {
 const terminal=new Terminal({cols:entry.cols,rows:entry.rows,allowProposedApi:true});
 if(terminal._core.unicodeService.activeVersion!=='6') throw new Error('Unexpected Unicode provider');
 for(const step of entry.steps) {
  if(step.hex!==undefined) await new Promise(resolve=>terminal.write(Buffer.from(step.hex,'hex'),resolve));else terminal.resize(...step.resize);
  const buffer=terminal.buffer.active;
  const allRows=Array.from({length:buffer.length},(_,y)=>Array.from({length:terminal.cols},(_,x)=>{const c=buffer.getLine(y).getCell(x);return {text:c.getChars(),width:c.getWidth()};}));
  step.expected={cursor:[buffer.cursorX,buffer.cursorY],join:terminal._core._inputHandler._parser.precedingJoinState,baseY:buffer.baseY,rows:allRows.slice(buffer.baseY,buffer.baseY+terminal.rows),allRows,copy:Array.from({length:buffer.length},(_,y)=>buffer.getLine(y).translateToString(true)).join('\n').trimEnd()};
 }
 terminal.dispose();
}
writeFileSync(destination,JSON.stringify(cases)+'\n');
console.log(JSON.stringify({cases:cases.length,cuts:cases.reduce((n,c)=>n+c.steps.length,0)}));
