// Exact shipped-xterm ALT backing/edit oracle, including retained off-grid cells.
import {createRequire} from 'node:module';
import {readFileSync,writeFileSync} from 'node:fs';
const require=createRequire(import.meta.url);const {Terminal}=require('@xterm/headless');
const [baseFile,destination]=process.argv.slice(2);if(!baseFile||!destination) throw new Error('usage: node alt-storage-cases.mjs base25.json output.json');
const write=s=>({hex:Buffer.from(s).toString('hex')});const resize=(cols,rows)=>({resize:[cols,rows]});const cup=(x,y=1)=>write(`\x1b[${y};${x}H`);
const alt=s=>write('\x1b[?1049h'+s);const cases=JSON.parse(readFileSync(baseFile)).map(c=>({...c,steps:c.steps.map(({hex,resize})=>hex===undefined?{resize}:{hex})}));
for(const [name,edit] of [['ICH','\x1b[1@'],['DCH','\x1b[2P'],['ECH','\x1b[5X'],['EL0','\x1b[K'],['EL2','\x1b[2K'],['ED0','\x1b[J'],['ED2','\x1b[2J']]) cases.push({name:'hidden-'+name,cols:8,rows:3,attributes:true,steps:[alt('ABCDEFGH'),resize(4,3),cup(2),write(edit),resize(8,3),write('Z')]});
cases.push(
 {name:'hidden-wide-intermediate-grow',cols:8,rows:3,attributes:true,steps:[alt('ABCDE中'),resize(4,3),resize(6,3),cup(2),write('\x1b[P'),cup(6),write('Q'),resize(8,3),write('Z')]},
 {name:'hidden-wide-zero-style-link',cols:8,rows:3,attributes:true,steps:[alt(''),write('\x1b[31;1;48;2;1;2;3m'),write('\x1b]8;;https://example.invalid/retained\x1b\\'),write('中\u0301ABCD'),write('\x1b]8;;\x1b\\'),write('\x1b[0m'),resize(4,3),cup(2),write('\x1b[2P'),write('\x1b[2@'),write('\x1b[3X'),resize(8,3),write('Z')]},
 {name:'hidden-rows-grow-scroll',cols:8,rows:3,attributes:true,steps:[alt('ABCDEFGH\r\nIJKLMNOP'),resize(4,4),cup(1,4),write('ROW'),write('\r\nNEW'),resize(8,4),write('Z')]},
 {name:'hidden-scroll-up-down',cols:8,rows:3,attributes:true,steps:[alt('ABCDEFGH\r\nIJKLMNOP'),resize(4,3),write('\x1b[S'),write('\x1b[T'),resize(8,3),write('Z')]},
 {name:'hidden-insert-delete-lines',cols:8,rows:3,attributes:true,steps:[alt('ABCDEFGH\r\nIJKLMNOP'),resize(4,3),cup(1),write('\x1b[L'),write('\x1b[M'),resize(8,3),write('Z')]},
 {name:'hidden-erase-colored-ragged',cols:8,rows:3,attributes:true,steps:[alt('ABCDEFGH'),resize(4,4),write('\x1b[44m'),write('\x1b[2J'),resize(6,4),write('\x1b[0mZ')]},
 {name:'hidden-wide-wrap-live',cols:8,rows:3,attributes:true,steps:[alt('AB中EFGH'),resize(4,3),cup(4),write('界\u0301'),resize(8,3),write('Z')]},
 {name:'hidden-wide-base-overwrite',cols:8,rows:3,attributes:true,steps:[alt('ABC中EFG'),resize(4,3),cup(4),write('X'),resize(8,3),write('Z')]},
 {name:'resize-join-width-state',cols:8,rows:3,steps:[write('ABC中'),resize(4,3),write('\u0301'),write('X')]}
);
function cell(t,buffer,y,x) {
 const line=buffer.getLine(y),c=line.getCell(x);if(!c)return {text:'',width:1};
 const raw=t._core._bufferService.buffer.lines.get(y);const urlId=(raw.getBg(x)&0x10000000)?raw._extendedAttrs[x]?.urlId:undefined;
 return {text:c.getChars(),width:c.getWidth(),fgMode:c.getFgColorMode()>>>24,fg:c.getFgColorMode()?c.getFgColor():0,bgMode:c.getBgColorMode()>>>24,bg:c.getBgColorMode()?c.getBgColor():0,bold:!!c.isBold(),uri:urlId?(t._core._oscLinkService.getLinkData(urlId)?.uri??''):''};
}
for(const entry of cases) {
 const t=new Terminal({cols:entry.cols,rows:entry.rows,allowProposedApi:true});if(t._core.unicodeService.activeVersion!=='6')throw new Error('Not Unicode6');
 for(const step of entry.steps) {
  if(step.hex!==undefined) await new Promise(resolve=>t.write(Buffer.from(step.hex,'hex'),resolve));else t.resize(...step.resize);
  const b=t.buffer.active;const allRows=Array.from({length:b.length},(_,y)=>Array.from({length:t.cols},(_,x)=>cell(t,b,y,x)));
  const backingRows=Array.from({length:b.length},(_,y)=>Array.from({length:b.getLine(y).length},(_,x)=>cell(t,b,y,x)));
  step.expected={cursor:[b.cursorX,b.cursorY],join:t._core._inputHandler._parser.precedingJoinState,baseY:b.baseY,rows:allRows.slice(b.baseY,b.baseY+t.rows),allRows,backingRows,copy:Array.from({length:b.length},(_,y)=>b.getLine(y).translateToString(true)).join('\n').trimEnd()};
 }
 t.dispose();
}
writeFileSync(destination,JSON.stringify(cases)+'\n');console.log(JSON.stringify({cases:cases.length,cuts:cases.reduce((n,c)=>n+c.steps.length,0)}));
