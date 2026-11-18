// REVIEW-ONLY projection over actual source oracle records, never a native API,
// decoder, producer adapter, admission implementation or fallback from defaults.
import assert from 'node:assert/strict';
export function compareSavedMetadataSourceCapture(expected,actual){
 for(const key of ['active','currentAttr','charset','modes','join'])assert.deepEqual(actual[key],expected[key],key);
 for(const key of ['primary','alternate']){
  const e=expected[key],a=actual[key];assert.ok(a,key);
  for(const field of ['x','y','base','lineCount','savedX','savedBufferY','savedAttr','savedCharset','rows'])assert.deepEqual(a[field],e[field],`${key}.${field}`);
  assert.equal(typeof a.copy,'string',`${key}.copy missing`);assert.equal(a.copy,e.copy,`${key}.copy bytes`);
 }
}
const uint32=n=>Number.isInteger(n)&&n>=0&&n<=0xffffffff;
export function reviewSavedFgBgCharsetV1(capture,closedCharsets){
 const unknown=()=>({descriptorStatus:'state_unknown',completeCaptureStatus:'state_unknown',reason:'missing_actual_source_metadata'});
 if(!capture?.primary?.savedAttr||!capture?.alternate?.savedAttr||!capture?.charset||!capture.currentAttr||!Array.isArray(closedCharsets))return unknown();
 const token=value=>value&&closedCharsets.some(c=>c.kind===value.kind&&c.mapSha256===value.mapSha256);
 const descriptors={};
 for(const name of ['primary','alternate']){
  const b=capture[name],a=b.savedAttr;
  if(!uint32(a.fg)||!uint32(a.bg)||!b.savedCharset)return unknown();
  if((a.bg&0x80000000)!==0)return {descriptorStatus:'unsupported',completeCaptureStatus:'unsupported',reason:'source_bg_reserved_bit'};
  if(!token(b.savedCharset))return {descriptorStatus:'unsupported',completeCaptureStatus:'unsupported',reason:'charset_outside_closed_fingerprints'};
  // Shipped save never writes saved extended. Preserve/assert that source fact,
  // do not invent a saved extended clone from a native full style object.
  if(a.storedExtended!==0||a.urlId!==0)return {descriptorStatus:'unsupported',completeCaptureStatus:'unsupported',reason:'unproven_saved_extended_payload'};
  descriptors[name]={type:'SavedFgBgCharsetV1',rawfg_u32:a.fg,rawbg_u32:a.bg,saved_active_charset_id:b.savedCharset.kind};
 }
 const result={descriptorStatus:'representable_source_grammar',descriptors,completeCaptureStatus:'state_unknown',reason:'native_equivalence_and_boundary_not_implemented'};
 const ch=capture.charset,pen=capture.currentAttr;
 if(!token(ch.fallback)||ch.fallback.kind!=='Undefined')return {...result,completeCaptureStatus:'unsupported',reason:'nonstock_or_unproven_global_fallback'};
 if(!token(ch.effective)||!Array.isArray(ch.banks)||ch.banks.length!==4||!ch.banks.every(token)||!Number.isInteger(ch.glevel)||ch.glevel<0||ch.glevel>3)return {...result,completeCaptureStatus:'unsupported',reason:'current_charset_machine_not_closed'};
 if(!uint32(pen.fg)||!uint32(pen.bg))return unknown();
 if((pen.bg&0x80000000)!==0)return {...result,completeCaptureStatus:'unsupported',reason:'current_bg_reserved_bit'};
 if(pen.storedExtended!==0||pen.urlId!==0||pen.hasExtended||Object.values(descriptors).some(d=>(d.rawbg_u32&0x10000000)!==0))return {...result,completeCaptureStatus:'unsupported',reason:'separate_current_extended_dependency_not_admitted'};
 return result;
}
