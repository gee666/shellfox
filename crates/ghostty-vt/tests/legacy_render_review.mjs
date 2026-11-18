// Read-only structural review of the exact shipped DOM renderer, not pixels/AT.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
const require = createRequire(import.meta.url);
const { chromium } = require('@playwright/test');
const bundle = require.resolve('@xterm/xterm');
const packageRoot = resolve(dirname(bundle), '..');
const pins = {
  'lib/xterm.js': '14903579ff54664cd72f8e8699e6961a6272c21863ec1c3b118cdc8af5d4a972',
  'css/xterm.css': '854a7c0fb70e8b1a083c16797ab827299fb18744f5ad34f227b48337e33293c6',
  'src/browser/OscLinkProvider.ts': '43f52cd9b945b4e9001a48c68f4b5de492d9579609e5f2800dbd7ce8a0f5a470',
  'src/browser/services/SelectionService.ts': '0bd2c55a0119dd91549b5d62778c649ec45f3c8aecf160be6324e5e2590a3d7c',
  'src/browser/services/MouseService.ts': 'da261e5f77dad78cafdfe7b8ef079b3702b49d4cfacca595fe6b56f675bd2572',
  'src/browser/renderer/dom/DomRendererRowFactory.ts': '1976e53ad3aac9fcc234f9eafb4ed188ab939297fe457045b0372c53a93298cc',
  'src/browser/renderer/dom/DomRenderer.ts': '4966d79e57629b5e31faf8b9fd64bd72f73c4170f9d913653b583c4ef7770869',
  'src/browser/Linkifier.ts': 'b53e05a656bc725f12ff7019cca9990568a4f038d42fbe9575a7e36779fa328a',
  'src/browser/input/Mouse.ts': '790d3feed4b91d02a40cecf6eae7fdd422f7c7aec64fdb7245c123699d011264',
};
for (const [file, digest] of Object.entries(pins)) {
  assert.equal(createHash('sha256').update(readFileSync(join(packageRoot,file))).digest('hex'), digest, file);
}
assert.equal(JSON.parse(readFileSync(join(packageRoot,'package.json'))).version,'6.0.0');
const tmp=resolve('tmp');mkdirSync(tmp,{recursive:true});
const profile=mkdtempSync(join(tmp,'maurice-render-review-'));
let context;
const watchdog=setTimeout(()=>{console.error('structural review timeout');Promise.resolve(context?.close()).finally(()=>{
  rmSync(profile,{recursive:true,force:true});process.exit(2);
});},30000);watchdog.unref();
try {
  context=await chromium.launchPersistentContext(profile,{headless:true,viewport:{width:800,height:500}});
  const page=await context.newPage();
  await page.setContent('<div id="terminal" style="width:600px;height:400px"></div>');
  await page.addStyleTag({path:join(packageRoot,'css/xterm.css')});
  await page.addScriptTag({path:bundle});
  async function setup(cols=8) {
    await page.evaluate(async cols=>{
      window.term?.dispose();document.getElementById('terminal').innerHTML='';window.activations=[];
      window.term=new Terminal({cols,rows:3,allowProposedApi:true,fontFamily:'monospace',fontSize:16,
        cursorBlink:false,logLevel:'off',linkHandler:{activate:(event,uri)=>activations.push({uri,targetClass:event.target.className})}});
      term.open(document.getElementById('terminal'));term.focus();
      await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
    },cols);
  }
  async function write(bytes) {
    await page.evaluate(async bytes=>{
      await new Promise(resolve=>term.write(bytes,resolve));
      await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
    },bytes);
  }
  async function state() {
    return page.evaluate(()=>{
      const row=document.querySelector('.xterm-rows>div');const bounds=row.getBoundingClientRect();
      const width=term._core._renderService.dimensions.css.cell.width;
      return {cursor:[term.buffer.active.cursorX,term.buffer.active.cursorY],
        cells:Array.from({length:term.cols},(_,x)=>{const c=term.buffer.active.getLine(0).getCell(x);return {x,text:c.getChars(),width:c.getWidth(),bg:c.getBgColor()};}),
        dom:row.textContent,overflow:getComputedStyle(row).overflow,
        spans:Array.from(row.children,span=>({text:span.textContent,classes:span.className,left:(span.getBoundingClientRect().left-bounds.left)/width,
          width:span.getBoundingClientRect().width/width,decoration:span.style.textDecoration})),
        pointerEvents:getComputedStyle(row.parentElement).pointerEvents,
        cursorSpans:row.querySelectorAll('.xterm-cursor').length};
    });
  }
  await setup();await write('\x1b[41m\u0301\x1b[0mA');
  const zero=await state();
  assert.equal(zero.cells[0].width,0);assert.equal(zero.cells[0].text,'\u0301');
  assert.equal(zero.cursor[0],2);assert(!zero.dom.includes('\u0301'));
  assert(zero.spans[0].text.startsWith('A'));assert(Math.abs(zero.spans[0].left)<0.05);
  assert(zero.spans.every(span=>!span.classes.includes('xterm-bg-1')),'zero cell creates no red background span');
  await write('\x1b[1;1H');
  const hiddenCursor=await state();assert.equal(hiddenCursor.cursor[0],0);assert.equal(hiddenCursor.cursorSpans,0);

  await setup();await write('A中B');
  const normalWide=await state();assert.equal(normalWide.cells[1].width,2);assert.equal(normalWide.cells[2].width,0);
  assert(normalWide.dom.includes('A中B'),'normal structural tail skipped without dropping following B');

  await setup();await write('\x1b[?1049hABC\x1b[41m中\x1b[0mEFG');
  await page.evaluate(()=>term.resize(4,3));await write('\x1b[1;4HX');
  await page.evaluate(()=>term.resize(8,3));await write('\x1b[1;5H');
  const stub=await state();assert.equal(stub.cells[4].width,0);assert.equal(stub.cells[4].text,'');
  assert.equal(stub.cells[3].text,'X');assert.equal(stub.cells[3].width,1);
  assert.equal(stub.cursor[0],4);assert.equal(stub.cursorSpans,0);
  assert(stub.dom.includes('ABCXEFG'));assert(stub.spans.every(span=>!span.classes.includes('xterm-bg-1')));

  await setup();await write('\u0301AB');
  const selections=await page.evaluate(()=>{
    term.select(0,0,1);const zero={text:term.getSelection(),range:term.getSelectionPosition()};
    term.select(1,0,1);const a={text:term.getSelection(),range:term.getSelectionPosition()};
    term.clearSelection();return {zero,a};
  });
  assert.equal(selections.zero.text,'\u0301');assert.equal(selections.a.text,'A');
  assert.equal(selections.a.range.start.x,1);assert.equal(selections.a.range.end.x,2);
  let mouseBox=await page.locator('.xterm-screen').boundingBox();
  let mouseSize=await page.evaluate(()=>term._core._renderService.dimensions.css.cell);
  await page.mouse.move(mouseBox.x+.1*mouseSize.width,mouseBox.y+.5*mouseSize.height);
  await page.mouse.down();
  await page.mouse.move(mouseBox.x+1.6*mouseSize.width,mouseBox.y+.5*mouseSize.height);
  await page.mouse.up();
  const drag=await page.evaluate(()=>({text:term.getSelection(),range:term.getSelectionPosition()}));
  assert.equal(drag.text,'A');assert.equal(drag.range.start.x,1);assert.equal(drag.range.end.x,2);
  await page.evaluate(()=>{term.clearSelection();window.mouseReports=[];term.onData(value=>mouseReports.push(value));});
  await write('\x1b[?1000h\x1b[?1006h');
  await page.mouse.click(mouseBox.x+.5*mouseSize.width,mouseBox.y+.5*mouseSize.height);
  const mouse=await page.evaluate(()=>mouseReports);
  assert(mouse.includes('\x1b[<0;1;1M'),'visualcol0 sends logicalgrid1-basedcol1, NOT A sourcecol2');
  assert(mouse.includes('\x1b[<0;1;1m'));

  await setup();await write('\x1b[?1049hABCDE中');
  await page.evaluate(()=>term.resize(4,3));await page.evaluate(()=>term.resize(6,3));
  await write('\x1b[44m\x1b[1;2H\x1b[P\x1b[?25l');
  const orphan=await state();
  assert.equal(orphan.cells[4].text,'中');assert.equal(orphan.cells[4].width,2);
  assert.equal(orphan.cells[5].width,1,'following physical slot is NOT a tail');
  assert.equal(orphan.overflow,'hidden');
  assert(orphan.spans.some(span=>span.classes.includes('xterm-bg-4')&&span.left>=5.9),'following background starts after wide span, not physical col5');
  await page.evaluate(()=>term.resize(8,3));await write('\x1b[1;7HQ');
  const followed=await state();assert.equal(followed.cells[4].width,2);assert.equal(followed.cells[6].text,'Q');
  const q=await page.evaluate(()=>{
    const row=document.querySelector('.xterm-rows>div'),w=term._core._renderService.dimensions.css.cell.width;
    const walker=document.createTreeWalker(row,NodeFilter.SHOW_TEXT);let node;
    while(node=walker.nextNode()){const index=node.textContent.indexOf('Q');if(index>=0){const r=document.createRange();r.setStart(node,index);r.setEnd(node,index+1);
      return (r.getBoundingClientRect().left-row.getBoundingClientRect().left)/w;}}
    return null;
  });
  assert(q!==null&&q>6.8&&q<7.2,'following Q flows to visualcol7 while stored atgridcol6');

  await setup();await write('\u0301\x1b]8;;https://example.invalid/review\x1b\\A\x1b]8;;\x1b\\B');
  const box=await page.locator('.xterm-screen').boundingBox();
  const size=await page.evaluate(()=>term._core._renderService.dimensions.css.cell);
  async function click(col) {await page.mouse.move(box.x+(col+.5)*size.width,box.y+.5*size.height);
    await page.waitForTimeout(40);await page.mouse.click(box.x+(col+.5)*size.width,box.y+.5*size.height);}
  await click(0);assert.equal((await page.evaluate(()=>activations)).length,0,'visual A does not change logical linkifier range');
  await click(1);
  const hit=await page.evaluate(()=>({events:activations,current:term._core._linkifier.value?._currentLink?.link?.range}));
  assert.equal(hit.events.length,1,'logical source-grid link range activates');
  assert(hit.events[0].targetClass.includes('xterm-screen'),'row spans have pointer-events:none');
  assert.deepEqual(hit.current,{start:{x:2,y:1},end:{x:2,y:1}});
  await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
  const linkPaint=await state();assert.equal(linkPaint.pointerEvents,'none');
  assert(linkPaint.spans.some(span=>span.text==='A'&&span.left<0.05&&span.classes.includes('xterm-underline-')));
  assert(linkPaint.spans.some(span=>span.text==='B'&&span.left>0.9&&span.left<1.1));
  console.log(JSON.stringify({scope:'structural DOM only; no glyph pixels/native AT',zero,hiddenCursor,normalWide,stub,selections,drag,mouse,orphan,followed,qPaintColumn:q,hit,linkPaint,
    hyperlink:'visual0 inactive; logicalgrid1 active; no URL opened'},null,2));
} finally {
  try { await context?.close(); }
  finally { clearTimeout(watchdog);rmSync(profile,{recursive:true,force:true}); }
}
