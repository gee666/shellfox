// Joint proposal audit only. Explicit draft token switches, not production ABI.
import assert from 'node:assert/strict';
import test from 'node:test';
import {readFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
const root='crates/ghostty-vt-sys/build-support/patches/terminal-profile/prototype/saved-metadata-plan/';
const hash=b=>createHash('sha256').update(b).digest('hex');
function canonicalKindForProposedToken(token){
 switch(token){case 1:return 'Undefined';case 2:return 'DecSpecial';case 3:return 'Uk';case 4:return 'Dutch';case 5:return 'Finnish';case 6:return 'French';case 7:return 'FrenchCanadian';case 8:return 'German';case 9:return 'Italian';case 10:return 'NorwegianDanish';case 11:return 'Spanish';case 12:return 'Swedish';case 13:return 'Swiss';default:throw Error('Invalid/unassigned proposal token');}
}
test('joint registry files have the exact supplied bytes and hashes',()=>{
 for(const [name,digest]of [
  ['registry-v1-proposed-tokens.preimage.txt','76adfa991ecd8eefcc815766ee43a48ace756ee1531634e0650ff53bdd7288ae'],
  ['registry-v1-proposed-tokens.review.json','70ddacfc80d31f1af7babd5810e11aafe0043bffacab2197a43199add344677e'],
  ['REGISTRY-PREIMAGE.md','b7888b4bc135a09cbcb48356d2b2396e10d24bb978cf0556176fff9a2827183d'],
  ['oracle.json','05080e3ed7def47985085f4205b697f6ad72bffd059be32ac14665f1f996a596'],
 ])assert.equal(hash(readFileSync(root+name)),digest,name);
 const bytes=readFileSync(root+'registry-v1-proposed-tokens.preimage.txt');assert.equal(bytes.length,988);assert.equal(bytes[bytes.length-1],10);assert.ok(!bytes.includes(13));assert.notDeepEqual([...bytes.subarray(0,3)],[239,187,191]);assert.equal(bytes.toString().split('\n').length,14);
});
test('all thirteen map digests and literal token lines derive from unchanged actual oracle',()=>{
 const oracle=JSON.parse(readFileSync(root+'oracle.json')),review=JSON.parse(readFileSync(root+'registry-v1-proposed-tokens.review.json'));
 assert.equal(oracle.knownCharsets.length,13);assert.equal(review.entries.length,13);assert.equal(review.proposed_invalid_token,0);
 let generated='';for(let token=1;token<=13;token++){
  const name=canonicalKindForProposedToken(token),source=oracle.knownCharsets.find(m=>m.kind===name),entry=review.entries[token-1];assert.ok(source,name);
  if(source.map!==null){assert.ok(Array.isArray(source.map));const keys=source.map.map(p=>p[0]);assert.equal(new Set(keys).size,keys.length);assert.deepEqual(keys,[...keys].sort());for(const pair of source.map){assert.equal(pair.length,2);assert.equal(typeof pair[0],'string');assert.equal(typeof pair[1],'string');}}
  const mapHash=hash(Buffer.from(JSON.stringify(source.map),'utf8'));assert.equal(mapHash,source.mapSha256);assert.equal(entry.proposed_token,token);assert.equal(entry.canonical_kind,name);assert.equal(entry.map_sha256,mapHash);generated+=`${token}:${name}:${mapHash}\n`;
 }
 assert.deepEqual(Buffer.from(generated,'utf8'),readFileSync(root+'registry-v1-proposed-tokens.preimage.txt'));
 assert.equal(hash(Buffer.from(generated,'utf8')),'76adfa991ecd8eefcc815766ee43a48ace756ee1531634e0650ff53bdd7288ae');
 assert.equal(hash(Buffer.from('null')),'74234e98afe7498fb5daf1f36ac2d78acc339464f950703b8c019892f982b90b');
 assert.equal(oracle.knownCharsets.find(m=>m.kind==='Dutch').map.find(p=>p[0]==='[')[1],'ij');
});
test('pretty aliases, token zero and unknown never substitute for literal source registry',()=>{
 for(const token of [0,14,-1,0xffffffff])assert.throws(()=>canonicalKindForProposedToken(token));
 const bytes=readFileSync(root+'registry-v1-proposed-tokens.preimage.txt','utf8'),expected=hash(Buffer.from(bytes));
 for(const [literal,alias]of [[':Undefined:',':KnownUndefined:'],[':DecSpecial:',':DEC:'],[':Uk:',':UK:']])assert.notEqual(hash(Buffer.from(bytes.replace(literal,alias))),expected);
});
