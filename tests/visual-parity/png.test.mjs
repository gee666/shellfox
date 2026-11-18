import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {decodePng,encodePng,compareImages} from './png.mjs';
test('PNG roundtrip preserves every channel and detects a one-channel difference',()=>{
  const image={width:2,height:2,pixels:Buffer.from([0,0,0,255,255,0,0,255,0,255,0,128,0,0,255,0])};
  const decoded=decodePng(encodePng(image));
  assert.deepEqual(decoded,image);
  assert.equal(compareImages(image,decoded).equal,true);
  const changed={...decoded,pixels:Buffer.from(decoded.pixels)};changed.pixels[0]=1;
  const diff=compareImages(image,changed);
  assert.equal(diff.equal,false);assert.equal(diff.changedPixels,1);assert.equal(diff.maxChannelDelta,1);
  assert.deepEqual(diff.bounds,{x:0,y:0,width:1,height:1});
});
test('different dimensions fail without resizing or silently cropping',()=>{
  const a={width:1,height:1,pixels:Buffer.from([1,2,3,255])};
  const b={width:2,height:1,pixels:Buffer.from([1,2,3,255,1,2,3,255])};
  const compared=compareImages(a,b);assert.equal(compared.dimensionsEqual,false);assert.equal(compared.equal,false);assert.equal(compared.changedPixels,1);
});
test('malformed PNGs and corrupt CRCs fail explicitly',()=>{
  assert.throws(()=>decodePng(Buffer.from('not png')));
  const bytes=encodePng({width:1,height:1,pixels:Buffer.from([1,2,3,255])});bytes[20]^=1;
  assert.throws(()=>decodePng(bytes),/CRC/);
});
test('actual Electron/System.Drawing PNGs decode without pixel conversion',async()=>{
  const root=new URL('./baselines/windows-x64-dpi96/electron-reference-final/',import.meta.url);
  for(const kind of ['renderer','client','window']) {
    const image=decodePng(await readFile(new URL('01-empty.'+kind+'.png',root)));
    assert.equal(image.width,kind==='window'?1216:1200);
    assert.equal(image.height,kind==='window'?799:760);
    assert.equal(compareImages(image,decodePng(encodePng(image))).equal,true);
  }
});
