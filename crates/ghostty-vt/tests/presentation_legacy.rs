//! Public DOM-xterm comparison. Run explicitly with --ignored after installing
//! this repository's Playwright Chromium. Never downloads browsers during tests.
use shellfox_ghostty_vt::{Selection, Terminal, ViewportPoint};
use std::process::Command;

type TestResult = Result<(), Box<dyn std::error::Error>>;
fn hex(value: &str) -> String {
    const DIGITS: &[u8] = b"0123456789abcdef";
    value
        .bytes()
        .flat_map(|b| {
            [
                char::from(DIGITS[usize::from(b >> 4)]),
                char::from(DIGITS[usize::from(b & 15)]),
            ]
        })
        .collect()
}
const DOM: &str = r#"
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const {chromium}=require('@playwright/test');
const [word,line,wide,scrolled,reflow]=process.argv.slice(1).map(x=>Buffer.from(x,'hex').toString('utf8'));
let context;
const watchdog=setTimeout(()=>{console.error('presentation xterm timeout');
  Promise.resolve(context?.close()).finally(()=>process.exit(2));},25000);watchdog.unref();
(async()=>{
  const root=path.join(process.cwd(),'tmp');fs.mkdirSync(root,{recursive:true});
  const profile=fs.mkdtempSync(path.join(root,'presentation-xterm-'));
  try {
    context=await chromium.launchPersistentContext(profile,{headless:true,viewport:{width:800,height:500}});
    const page=await context.newPage();
    await page.setContent('<div id="terminal" style="width:600px;height:400px"></div>');
    await page.addStyleTag({path:require.resolve('@xterm/xterm/css/xterm.css')});
    await page.addScriptTag({path:require.resolve('@xterm/xterm')});
    async function setup(data,cols,rows) {
      await page.evaluate(async({data,cols,rows})=>{
        window.term?.dispose();document.getElementById('terminal').innerHTML='';
        window.term=new Terminal({cols,rows,scrollback:3000,disableStdin:true,allowProposedApi:true,
          fontFamily:'monospace',fontSize:14,logLevel:'off'});
        window.term.open(document.getElementById('terminal'));window.term.focus();
        await new Promise(resolve=>window.term.write(data,resolve));
        await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
      },{data,cols,rows});
    }
    async function click(col,row,count) {
      const rectangle=await page.locator('.xterm-screen').boundingBox();
      const dimensions=await page.evaluate(()=>({cols:term.cols,rows:term.rows}));
      await page.mouse.click(rectangle.x+(col+.5)*rectangle.width/dimensions.cols,
        rectangle.y+(row+.5)*rectangle.height/dimensions.rows,{clickCount:count});
    }
    const fixture='cafe\u0301 界!\r\nabcdefghijk\r\nlast';
    await setup(fixture,8,4);await click(2,0,2);
    assert.equal(await page.evaluate(()=>term.getSelection()),word,'native Unicode word boundaries');
    await page.evaluate(()=>{term.clearSelection();term.select(5,0,2);});
    assert.equal(await page.evaluate(()=>term.getSelection()),wide,'wide-cell range');
    await page.evaluate(()=>term.clearSelection());await click(2,1,3);
    assert.equal(await page.evaluate(()=>term.getSelection()),line,'logical soft-wrapped line');
    await setup('first\r\nsecond\r\nthird\r\nfourth\r\nlast',12,3);
    await page.evaluate(()=>term.scrollToTop());
    const text=await page.evaluate(()=>Array.from({length:term.rows},(_,y)=>
      term.buffer.active.getLine(term.buffer.active.viewportY+y).translateToString(true)).join('\n'));
    assert.equal(text,scrolled,'historical viewport');
    await setup('one two\r\nthree\r\nfour\r\nfive',12,3);
    await page.evaluate(async()=>{term.resize(6,4);term.scrollToTop();
      await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));});
    await click(4,0,2);
    assert.equal(await page.evaluate(()=>term.getSelection()),reflow,'word across a reflowed soft wrap');
    await page.evaluate(()=>term.dispose());
  } finally {
    await context?.close();fs.rmSync(profile,{recursive:true,force:true});clearTimeout(watchdog);
  }
})().catch(error=>{console.error(error);process.exitCode=1;});
"#;

#[test]
fn public_rows_keep_wrap_continuation_until_reflow() -> TestResult {
    let mut terminal = Terminal::new(8, 4)?;
    terminal.feed(b"abcdefghijkl\r\nhard")?;
    let mut view = terminal.new_presentation_view()?;
    // The same xterm fixture wraps the second row at 8 columns. At 16
    // columns the logical line fits, so none of the rows continue a wrap.
    let flags = |frame: shellfox_ghostty_vt::PresentationFrame| {
        frame
            .row_metadata
            .into_iter()
            .map(|row| row.wrap_continuation)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        flags(terminal.view_render(&mut view)?),
        [false, true, false, false]
    );
    terminal.resize(16, 4)?;
    assert_eq!(flags(terminal.view_render(&mut view)?), [false; 4]);
    Ok(())
}

#[test]
#[ignore = "requires existing Playwright Chromium; run this dedicated target with --ignored"]
fn real_dom_xterm_word_line_wide_history_and_reflow_differential() -> TestResult {
    let mut terminal = Terminal::new(8, 4)?;
    terminal.feed("cafe\u{301} 界!\r\nabcdefghijk\r\nlast".as_bytes())?;
    let mut view = terminal.new_presentation_view()?;
    terminal.view_scroll(&mut view, isize::MIN)?;
    terminal.view_select(
        &mut view,
        Some(Selection::Word(ViewportPoint { x: 2, y: 0 })),
    )?;
    let word = terminal.view_copy(&mut view)?.ok_or("missing word")?;
    terminal.view_select(
        &mut view,
        Some(Selection::Line(ViewportPoint { x: 2, y: 1 })),
    )?;
    let line = terminal.view_copy(&mut view)?.ok_or("missing line")?;
    terminal.view_select(
        &mut view,
        Some(Selection::Range {
            start: ViewportPoint { x: 5, y: 0 },
            end: ViewportPoint { x: 6, y: 0 },
            rectangle: false,
        }),
    )?;
    let wide = terminal
        .view_copy(&mut view)?
        .ok_or("missing wide selection")?;
    let mut terminal = Terminal::new(12, 3)?;
    terminal.feed(b"first\r\nsecond\r\nthird\r\nfourth\r\nlast")?;
    let mut view = terminal.new_presentation_view()?;
    terminal.view_scroll(&mut view, isize::MIN)?;
    let frame = terminal.view_render(&mut view)?;
    let scrolled = frame
        .screen
        .cells
        .chunks(12)
        .map(|row| {
            row.iter()
                .map(|c| c.text.as_str())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut terminal = Terminal::new(12, 3)?;
    terminal.feed(b"one two\r\nthree\r\nfour\r\nfive")?;
    let mut view = terminal.new_presentation_view()?;
    terminal.view_scroll(&mut view, isize::MIN)?;
    terminal.view_select(
        &mut view,
        Some(Selection::Word(ViewportPoint { x: 4, y: 0 })),
    )?;
    terminal.resize(6, 4)?;
    let reflow = terminal
        .view_copy(&mut view)?
        .ok_or("missing reflowed selection")?;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new("node")
        .current_dir(root)
        .args([
            "-e",
            DOM,
            &hex(&word),
            &hex(&line),
            &hex(&wide),
            &hex(&scrolled),
            &hex(&reflow),
        ])
        .output()?;
    assert!(
        output.status.success(),
        "DOM xterm differential failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
