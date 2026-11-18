import assert from 'node:assert/strict';
import test from 'node:test';
import {readFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {collectSavedMetadataOracle} from '../tools/saved-metadata-oracle.mjs';
import {reviewSavedFgBgCharsetV1,compareSavedMetadataSourceCapture} from '../tools/saved-metadata-contract.mjs';
let corpus;
const entry=name=>corpus.cases.find(c=>c.name===name);
const at=(name,cut)=>entry(name).steps[cut].expected;
const syntax=e=>reviewSavedFgBgCharsetV1(e,corpus.knownCharsets);
test('actual shipped source saved metadata oracle, not a native/format pass',async()=>{
 corpus=await collectSavedMetadataOracle();assert.deepEqual(corpus.summary,{cases:21,cuts:86,syntheticCases:2});assert.equal(corpus.knownCharsets.length,13);
 for(const c of corpus.cases)for(const s of c.steps)compareSavedMetadataSourceCapture(s.expected,structuredClone(s.expected));
 for(const c of corpus.cases)for(const s of c.steps)for(const key of ['primary','alternate']){
  const b=s.expected[key];assert.equal(typeof b.copy,'string');assert.equal(b.savedAttr.storedExtended,0);assert.equal(b.savedAttr.urlId,0);
 }
});
test('default-before-save uses source initialized words and fallback but current origin/banks',()=>{
 const e=at('default-before-save-current-attrs-origin-charset',1);assert.equal(e.primary.savedAttr.fg,0);assert.equal(e.primary.savedAttr.bg,0);assert.equal(e.primary.savedCharset.kind,'Undefined');assert.equal(e.currentAttr.fg,0);assert.equal(e.currentAttr.bg,0);assert.equal(e.modes.origin,true);assert.equal(e.primary.y,1);assert.equal(e.charset.effective.kind,'Undefined');assert.equal(e.charset.banks[0].kind,'DecSpecial');assert.equal(at('default-before-save-current-attrs-origin-charset',3).primary.copy,'\nq─');
});
test('saved raw fg/bg includes flags/protection and palette source mode',()=>{
 const saved=at('saved-fg-bg-flags-protection',0).primary.savedAttr,restored=at('saved-fg-bg-flags-protection',2).currentAttr;
 assert.equal(restored.fg,saved.fg);assert.equal(restored.bg,saved.bg);assert.equal(restored.protected,true);
 const c=entry('palette16-and-palette256-remain-distinct-source-modes');assert.notEqual(c.steps[0].expected.currentAttr.fg,c.steps[1].expected.currentAttr.fg);assert.equal(c.steps[2].expected.currentAttr.fg,c.steps[0].expected.primary.savedAttr.fg);
 const rgb=entry('saved-basic-RGB-and-known-flags');assert.equal(rgb.steps[2].expected.currentAttr.fg,rgb.steps[0].expected.primary.savedAttr.fg);assert.ok(rgb.steps[2].expected.currentAttr.fg>0x7fffffff);
});
test('restore changes effective map only, not current selector or designated banks',()=>{
 const name='saved-DEC-effective-map-does-not-restore-bank-or-selector',e=at(name,2);assert.equal(e.charset.effective.kind,'DecSpecial');assert.equal(e.charset.glevel,1);assert.equal(e.charset.banks[1].kind,'Uk');assert.equal(at(name,4).primary.copy,'─£');
 const u='saved-undefined-charset-does-not-restore-current-G1';assert.equal(at(u,2).charset.effective.kind,'Undefined');assert.equal(at(u,2).charset.glevel,1);assert.equal(at(u,2).charset.banks[1].kind,'DecSpecial');assert.equal(at(u,4).primary.copy,'q─');
});
test('extended payload stays CURRENT, saved only has fg/bg activation bits',()=>{
 const n='extended-save-red-curly-current-green-double',saved=at(n,0).primary.savedAttr,changed=at(n,1).currentAttr,restored=at(n,2).currentAttr;
 assert.equal(saved.storedExtended,0);assert.equal(saved.hasExtended,true);assert.equal(restored.storedExtended,changed.storedExtended);assert.equal(restored.underlineStyle,2);assert.equal(restored.underlineColor,0x0ac81e);
 const x='saved-extended-flag-current-payload-reset';assert.equal(at(x,2).currentAttr.hasExtended,true);assert.equal(at(x,2).currentAttr.storedExtended,0);assert.equal(at(x,2).currentAttr.isUnderline,true);assert.equal(at(x,2).currentAttr.underlineStyle,0);
 const latent='saved-basic-current-latent-extended-payload';assert.equal(at(latent,2).currentAttr.hasExtended,false);assert.notEqual(at(latent,2).currentAttr.storedExtended,0);assert.equal(at(latent,4).currentAttr.underlineColor,0x0ac81e);
 assert.equal(syntax(at(n,2)).descriptorStatus,'representable_source_grammar');assert.equal(syntax(at(n,2)).completeCaptureStatus,'unsupported');
});
test('saved link flag can activate current link, latent current URI must not be dropped',()=>{
 const a=at('saved-URL-A-current-URL-B',3);assert.equal(a.primary.rows[0].cells[0].activeUri,'https://example.invalid/B');assert.equal(a.primary.savedAttr.urlId,0);
 const n='saved-basic-current-latent-URL-B';assert.equal(at(n,3).primary.rows[0].cells[0].activeUri,null);assert.equal(at(n,4).primary.rows[0].cells[1].activeUri,'https://example.invalid/B');assert.equal(syntax(at(n,3)).completeCaptureStatus,'unsupported');
});
test('per-buffer saved words and charset survive ALT clear',()=>{
 const n='per-buffer-saved-attrs-charset-and-ALT-clear',e=at(n,3);assert.notEqual(e.primary.savedAttr.fg,e.alternate.savedAttr.fg);assert.equal(e.primary.savedCharset.kind,'DecSpecial');assert.equal(e.alternate.savedCharset.kind,'Uk');assert.equal(e.alternate.lineCount,0);assert.equal(at(n,4).alternate.copy,'£');
});
test('restore uses CURRENT origin and does not restore other current modes',()=>{
 const off=at('save-origin-on-restore-uses-current-origin-off',2),on=at('save-origin-off-restore-uses-current-origin-on',2);assert.equal(off.modes.origin,false);assert.equal(off.primary.y,2);assert.equal(on.modes.origin,true);assert.equal(on.primary.y,1);
 const e=at('restore-does-not-restore-wrap-reverse-or-insert-modes',2);assert.equal(e.modes.wraparound,false);assert.equal(e.modes.reverseWraparound,true);assert.equal(e.modes.insert,true);
});
test('DECSTR replaces active saved metadata; RIS recreates both banks',()=>{
 const soft=at('DECSTR-replaces-active-saved-attrs-and-charset',1);assert.equal(soft.primary.savedAttr.fg,0);assert.equal(soft.primary.savedAttr.bg,0);assert.equal(soft.primary.savedCharset.kind,'Undefined');assert.equal(at('DECSTR-replaces-active-saved-attrs-and-charset',3).primary.copy,'q');
 const reset=at('RIS-recreates-both-saved-banks',1);for(const key of ['primary','alternate']){assert.equal(reset[key].savedAttr.fg,0);assert.equal(reset[key].savedAttr.bg,0);assert.equal(reset[key].savedCharset.kind,'Undefined');}assert.equal(at('RIS-recreates-both-saved-banks',2).primary.copy,'q');
});
test('actual fallback presence/value is recorded, synthetic paths are not stock admission',()=>{
 for(const c of corpus.cases.filter(c=>!c.synthetic))for(const s of c.steps){assert.equal(s.expected.charset.fallbackPropertyPresent,false);assert.equal(s.expected.charset.fallback.kind,'Undefined');}
 const e=at('synthetic-global-fallback-DEC',2);assert.equal(e.charset.fallbackPropertyPresent,true);assert.equal(e.charset.fallback.kind,'DecSpecial');assert.equal(e.primary.copy,'─');assert.equal(syntax(e).completeCaptureStatus,'unsupported');
});
test('closed fingerprints do not invent native equivalence or accept custom/missing state',()=>{
 const french=at('stock-French-needs-source-equivalence-not-ASCII',2);assert.equal(french.primary.copy,'à');assert.equal(french.primary.savedCharset.kind,'French');assert.equal(syntax(french).descriptorStatus,'representable_source_grammar');assert.equal(syntax(french).completeCaptureStatus,'state_unknown');
 const custom=at('synthetic-unknown-saved-charset-map',3);assert.equal(custom.primary.copy,'!');assert.equal(syntax(custom).descriptorStatus,'unsupported');
 assert.equal(syntax({saved_cursor:null}).completeCaptureStatus,'state_unknown');
});
test('source comparator rejects copy changes and full-pen/charset substitution',()=>{
 const e=at('extended-save-red-curly-current-green-double',2);
 const mutations=[
  [e,a=>{a.primary.copy+=' ';}],
  [e,a=>{a.currentAttr.storedExtended=at('extended-save-red-curly-current-green-double',0).currentAttr.storedExtended;}],
  [e,a=>{a.primary.savedAttr={};}],
  [at('saved-DEC-effective-map-does-not-restore-bank-or-selector',2),a=>{a.charset.effective=a.charset.banks[a.charset.glevel];}],
 ];
 for(const [sample,change]of mutations){const a=structuredClone(sample);change(a);assert.throws(()=>compareSavedMetadataSourceCapture(sample,a));}
});
test('no frozen producer or P6 candidate was changed',()=>{
 const root='crates/ghostty-vt-sys/build-support/patches/terminal-profile/prototype/';for(const name of ['native-002-p4-003','layout2-data-001']){const m=JSON.parse(readFileSync(root+name+'/checkpoint.json'));assert.equal(createHash('sha256').update(readFileSync(root+name+'/'+m.patch)).digest('hex'),m.sha256);}
});
