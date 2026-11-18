import {readFile,writeFile,mkdir} from 'node:fs/promises';
import path from 'node:path';
import {decodePng,encodePng,compareImages} from '../tests/visual-parity/png.mjs';
import {root,json,sha256} from './electron-parity-common.mjs';
const [expectedArg,actualArg,label]=process.argv.slice(2);
if(!expectedArg||!actualArg||!label||!/^[a-z0-9][a-z0-9-]{0,80}$/.test(label))throw new Error('Usage: node scripts/electron-parity-compare.mjs BASELINE-DIR CANDIDATE-DIR UNIQUE-REPORT-LABEL');
const expectedDir=path.resolve(expectedArg),actualDir=path.resolve(actualArg);
const expected=JSON.parse(await readFile(path.join(expectedDir,'manifest.json'),'utf8'));
const actual=JSON.parse(await readFile(path.join(actualDir,'manifest.json'),'utf8'));
if(expected.platform!==actual.platform||expected.arch!==actual.arch||expected.identity.dpi!==actual.identity.dpi)throw new Error('Platform/architecture/DPI mismatch; native baselines cannot be inferred or scaled.');
for(const manifest of [expected,actual])if(manifest.policy?.masks?.length||manifest.policy?.pixelTolerance!==0)throw new Error('Only literal zero-tolerance, unmasked comparisons are permitted.');
const output=path.join(root,'tests/visual-parity/reports',label);await mkdir(output,{recursive:false});
const results=[];
for(const state of expected.states)for(const kind of ['client','window']) {
  if(!/^[a-z0-9][a-z0-9-]*$/.test(state.name))throw new Error('Unsafe state name.');
  const file=state.name+'.'+kind+'.png';
  try {
    const actualState=actual.states.find(item=>item.name===state.name);
    if(!actualState)throw new Error('Required candidate state missing from manifest.');
    if(state.window.focused!==actualState.window.focused)throw new Error('Native focus state differs; do not normalize frame pixels.');
    const expectedBytes=await readFile(path.join(expectedDir,file)),actualBytes=await readFile(path.join(actualDir,file));
    if(sha256(expectedBytes)!==state.files[kind]||sha256(actualBytes)!==actualState.files[kind])throw new Error('Screenshot hash differs from recorded provenance.');
    const a=decodePng(expectedBytes),b=decodePng(actualBytes);
    const compared=compareImages(a,b);
    const {diff,...result}=compared;
    results.push({state:state.name,kind,expected:{width:a.width,height:a.height},actual:{width:b.width,height:b.height},...result});
    if(!result.equal)await writeFile(path.join(output,state.name+'.'+kind+'.diff.png'),encodePng(diff));
  } catch(error) {results.push({state:state.name,kind,equal:false,error:error.message});}
}
const pass=results.length>0&&results.every(result=>result.equal);
await json(path.join(output,'comparison.json'),{format:1,pass,rule:'Every decoded RGBA pixel identical; no tolerance, antialias exemption, masks, resize or missing-state skip.',expectedDir,actualDir,results});
console.log(JSON.stringify({pass,matched:results.filter(r=>r.equal).length,total:results.length,output},null,2));
if(!pass)process.exitCode=1;
