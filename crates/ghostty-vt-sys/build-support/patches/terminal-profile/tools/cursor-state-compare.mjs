// Test-only exact source-state comparator. Actual captures must supply retained
// source fields; there is intentionally no base+relative or visible-tab fallback.
import assert from 'node:assert/strict';
export function compareCursorSourceState(expected,actual){
 for(const name of ['cols','rows','active','wraparound','reverseWraparound','join'])assert.deepEqual(actual[name],expected[name],name);
 for(const key of ['primary','alternate']){
  const e=expected[key],a=actual[key];assert.ok(a&&e,key);
  for(const name of ['x','y','base','display','savedX','savedBufferY','scrollTop','scrollBottom','maxLines']){
   assert.ok(Object.hasOwn(a,name),`${key}.${name} missing, no inference allowed`);
   assert.deepEqual(a[name],e[name],`${key}.${name}`);
  }
  assert.ok(Array.isArray(a.tabs),`${key}.tabs missing`);
  assert.ok(a.tabs.every((v,i)=>Number.isSafeInteger(v)&&v>=0&&(i===0||a.tabs[i-1]<v)),`${key}.tabs must be exact sorted unique source keys`);
  assert.deepEqual(a.tabs,e.tabs,`${key}.tabs including off-grid keys`);
  assert.deepEqual(a.rows,e.rows,`${key}.rows text/width/wrap`);
  assert.equal(typeof a.copy,'string',`${key}.copy missing`);
  assert.equal(a.copy,e.copy,`${key}.copy bytes, including whitespace`);
 }
}
