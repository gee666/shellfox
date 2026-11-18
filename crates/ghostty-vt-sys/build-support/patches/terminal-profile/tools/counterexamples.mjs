import { createRequire } from 'node:module';
import { spawnSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';
const require=createRequire(import.meta.url);
const {Terminal}=require('@xterm/headless');
const [baselineExecutable,destination]=process.argv.slice(2);
if(!baselineExecutable||!destination) throw new Error('usage: node counterexamples.mjs verified-baseline-probe.exe report.json');
const probe=spawnSync(baselineExecutable,[],{encoding:'utf8',timeout:10000});
if(probe.status!==0) throw new Error(`Baseline probe failed: ${probe.stderr}`);
const native=probe.stdout.trim().split(/\r?\n/).map(line=>JSON.parse(line));
const inputs=['\u0301A','A\x1b[31m\u0301B','A\ufe0fB'];
const cases=[];
for(let i=0;i<inputs.length;i++) {
 const terminal=new Terminal({cols:8,rows:3,allowProposedApi:true});
 await new Promise(resolve=>terminal.write(inputs[i],resolve));
 const buffer=terminal.buffer.active,line=buffer.getLine(0);
 const cells=Array.from({length:8},(_,x)=>({x,text:line.getCell(x).getChars(),width:line.getCell(x).getWidth()}));
 cases.push({name:native[i].name,inputHex:Buffer.from(inputs[i]).toString('hex'),xterm:{visibleLine:line.translateToString(true),copyLineHex:Buffer.from(line.translateToString(true)).toString('hex'),cursor:[buffer.cursorX,buffer.cursorY],preceding_join:terminal._core._inputHandler._parser.precedingJoinState,cells},native:{cursor:native[i].cursor,copyLineHex:native[i].plainHex}});
 terminal.dispose();
}
writeFileSync(destination,JSON.stringify({baseline:'official signed ghostty-vt-1.0.1-x86_64-windows-gnu.zip',baselineSha256:'8e2eb9e1822e85d786b456c8beb86f49d09c318c91f0c2913f91e7ffa12cda29',oracle:'shipped @xterm/headless 6.0.0 UnicodeV6',cases},null,2)+'\n');
console.log(JSON.stringify(cases.map(c=>({name:c.name,xtermCursor:c.xterm.cursor,nativeCursor:c.native.cursor,xtermCopy:c.xterm.copyLineHex,nativeCopy:c.native.copyLineHex}))));
