// Actual shipped source oracle; P is measured by the private native test.
import {createRequire} from 'node:module';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {dirname,relative} from 'node:path';
const require=createRequire(import.meta.url),{Terminal}=require('@xterm/headless');
const P=Number(process.argv[2]),output=process.argv[3];
if(!Number.isInteger(P)||P<8||!output)throw Error('usage: node p4-005-oracle.mjs measured-P output');
const w=s=>({hex:Buffer.from(s).toString('hex')}),resize=rows=>({resize:[32,rows]});
function digest(t,b){
 const hash=createHash('sha256'),u32=v=>{const bytes=Buffer.alloc(4);bytes.writeUInt32LE(v);hash.update(bytes);};
 for(let y=0;y<b.lines.length;y++){
  const line=b.lines.get(y);hash.update(Buffer.from([+line.isWrapped]));
  for(let x=0;x<line.length;x++){
   const c=line.loadCell(x,t.buffer.active.getNullCell()),text=Buffer.from(c.getChars());u32(text.length);hash.update(text);hash.update(Buffer.from([c.getWidth()]));
   for(const fg of [true,false]){const mode=fg?(c.isFgDefault()?0:c.isFgPalette()?1:2):(c.isBgDefault()?0:c.isBgPalette()?1:2);hash.update(Buffer.from([mode]));u32(mode?(fg?c.getFgColor():c.getBgColor()):0);}
   // URL implies effective underline in isUnderline(); compare the stored FG
   // underline bit here and compare the owned cell URI independently below.
   hash.update(Buffer.from([+!!c.isBold(),+!!(c.fg&0x10000000)]));
   const id=c.hasExtendedAttrs()?c.extended.urlId:0,uri=id?t._core._oscLinkService.getLinkData(id)?.uri:undefined,link=Buffer.from(uri??'');u32(link.length);hash.update(link);
  }
 }
 return hash.digest('hex');
}
function capture(t,b){return {x:b.x,y:b.y,base:b.ybase,display:b.ydisp,savedX:b.savedX,savedBufferY:b.savedY,savedRawFG:b.savedCurAttrData.fg>>>0,savedRawBG:b.savedCurAttrData.bg>>>0,savedCharsetKnownUndefined:b.savedCharset===undefined,tabs:Object.keys(b.tabs).filter(k=>b.tabs[k]).map(Number).sort((a,b)=>a-b),length:b.lines.length,cellsSha256:digest(t,b),copy:Array.from({length:b.lines.length},(_,y)=>b.lines.get(y).translateToString(true)).join('\n').trimEnd()};}
function line(i,links){const label=`L${String(i).padStart(6,'0')}`;return '\x1b[38;2;17;34;51m\x1b[1m'+(links?'\x1b]8;;https://example.test/'+i+'\x1b\\':'')+label+'\u0301'+(links?'\x1b]8;;\x1b\\':'')+'\x1b[0m';}
const cases=[];
for(const links of [false,true])for(const H of [P-1,P,P+1,2*P+1]){
 let next=0;const steps=[];for(const end of [P-1,P+1,2*P+4,3*P+4]){let input='';for(;next<end;next++)input+=(next?'\r\n':'')+line(next,links);steps.push(w(input));if(end===P-1)steps.push(w('\x1b7'));}
 steps.push({scrollback:Math.max(0,H-Math.floor(P/2))},w('Q'),w('\x1b8'),w('X'),resize(2),resize(4),w('\r\nN\r\nM\x1b8Y'));
 cases.push({name:`native-P${P}-H${H}-${links?'managed-link-refusal':'styled-admitted'}`,cols:32,rows:4,scrollback:H,admit:!links,steps});
}
for(const key of ['primary','alternate','unfilled-alternate'])for(const cursor of [1,4]){
 const start=key==='primary'?'':'\x1b[?47h';
 const steps=[w(start+Array.from({length:6},(_,i)=>line(i,false)).join('\r\n')+'\x1b7'+`\x1b[${cursor};1H`)];
 if(key==='unfilled-alternate')steps.push(w('\x1b[?47l'));
 steps.push(resize(2),resize(4),w('\r\nN\r\nM'));
 if(key==='unfilled-alternate')steps.push(w('\x1b[?47h'));
 steps.push(w('\x1b8X'));
 cases.push({name:`full-history-height-shrink-${key}-cursor${cursor}`,cols:32,rows:4,scrollback:2,admit:true,steps});
}
let cuts=0;
for(const c of cases){const t=new Terminal({cols:c.cols,rows:c.rows,scrollback:c.scrollback,allowProposedApi:true});if(t._core.unicodeService.activeVersion!=='6')throw Error('unexpected Unicode source');for(const step of c.steps){if(step.hex!==undefined)await new Promise(done=>t.write(Buffer.from(step.hex,'hex'),done));else if(step.resize)t.resize(...step.resize);else t.options.scrollback=step.scrollback;const service=t._core._bufferService;step.expected={active:service.buffer===service.buffers.alt?'alternate':'primary',join:t._core._inputHandler._parser.precedingJoinState,primary:capture(t,service.buffers.normal),alternate:capture(t,service.buffers.alt)};cuts++;}t.dispose();}
const paths=[require.resolve('@xterm/headless'),'node_modules/@xterm/xterm/src/common/buffer/Buffer.ts','node_modules/@xterm/xterm/src/common/buffer/BufferLine.ts','node_modules/@xterm/xterm/src/common/InputHandler.ts'];
const sourceHashes=paths.map(p=>({path:relative(process.cwd(),p).replaceAll('\\','/'),sha256:createHash('sha256').update(readFileSync(p)).digest('hex')}));
mkdirSync(dirname(output),{recursive:true});writeFileSync(output,JSON.stringify({status:'PRIVATE005 actual source oracle, not native acceptance',nativeMeasuredPageRows:P,cellHashGrammar:'rowwrap:u8; each cell: UTF8len:u32le,text,width:u8,fgMode:u8,fg:u32le,bgMode:u8,bg:u32le,bold:u8,storedFGunderline:u8,URIlen:u32le,URI; modes0default1palette2RGB; all rows/all physical cells',sourceHashes,summary:{cases:cases.length,cuts},cases},null,2)+'\n');console.log(JSON.stringify({cases:cases.length,cuts,P,bytes:readFileSync(output).length}));
