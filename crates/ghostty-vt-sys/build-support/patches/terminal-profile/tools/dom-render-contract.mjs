// Structural/layout evidence from the shipped browser renderer, not pixel parity.
import { readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { pathToFileURL } from 'node:url';
import { chromium } from '@playwright/test';

const w = text => ({ text });
const resize = cols => ({ resize: cols });
const orphan = [w('\x1b[?1049hABCDE\x1b[41m中\x1b[0m'), resize(4), resize(6)];
export const fixtures = [
  { name: 'narrow', cols: 8, steps: [w('A\x1b[42mB')] },
  { name: 'wide-pair', cols: 8, steps: [w('A\x1b[41m中\x1b[42mB')] },
  { name: 'zero-text-leading-background', cols: 8, steps: [w('\x1b[41m\u0301\x1b[42mA')] },
  { name: 'zero-text-middle-background', cols: 8, steps: [w('A\x1b[41m\u0301\x1b[42mB')] },
  { name: 'orphan-right-clip', cols: 8, steps: orphan },
  { name: 'orphan-following-background-clipped', cols: 8, steps: [...orphan, w('\x1b[42m\x1b[1;2H\x1b[P')] },
  { name: 'orphan-following-glyph-clipped', cols: 8, steps: [...orphan, w('\x1b[42m\x1b[1;2H\x1b[P'), resize(8), w('\x1b[1;8HQ')], cursorX: 7 },
  { name: 'orphan-following-glyph-regrow', cols: 8, steps: [...orphan, w('\x1b[42m\x1b[1;2H\x1b[P'), resize(8), w('\x1b[1;8HQ'), resize(10)] },
  { name: 'cursor-narrow', cols: 8, steps: [w('A')], cursorX: 0 },
  { name: 'cursor-zero-text-skips-class', cols: 8, steps: [w('\u0301A')], cursorX: 0 },
  { name: 'cursor-wide-tail-skips-class', cols: 8, steps: [w('中A')], cursorX: 1 },
  { name: 'cursor-empty-zero-stub-skips-class', cols: 8, steps: [w('XA')], synthetic: 'zero-stub', cursorX: 0 },
  { name: 'zero-link-source-grid-versus-paint', cols: 8, steps: [w('\u0301\x1b]8;;https://example.invalid/source\x1b\\A\x1b]8;;\x1b\\B')] },
  { name: 'hidden-backing-enters-paint-after-zero', cols: 8, steps: [w('\x1b[?1049h\u0301ABC\x1b[41mDEF'), resize(4)] },
  { name: 'wide-wrap-padding', cols: 4, steps: [w('ABC\x1b[41m中')] },
  // Synthetic renderer inputs are labeled, not claimed reachable parser states.
  { name: 'empty-zero-stub-background', cols: 8, steps: [w('XA')], synthetic: 'zero-stub' },
  { name: 'wide-tail-independent-background', cols: 8, steps: [w('\x1b[44m中\x1b[42mB')], synthetic: 'tail-color' },
];
const sources = [
  'node_modules/@xterm/xterm/package.json',
  'node_modules/@xterm/xterm/lib/xterm.js',
  'node_modules/@xterm/xterm/css/xterm.css',
  'node_modules/@xterm/xterm/src/browser/renderer/dom/DomRendererRowFactory.ts',
  'node_modules/@xterm/xterm/src/browser/renderer/dom/DomRenderer.ts',
  'node_modules/@xterm/xterm/src/common/buffer/BufferLine.ts',
  'node_modules/@xterm/xterm/src/browser/Linkifier.ts',
  'node_modules/@xterm/xterm/src/browser/OscLinkProvider.ts',
  'node_modules/@xterm/xterm/src/browser/input/Mouse.ts',
  'node_modules/@xterm/xterm/src/browser/services/MouseService.ts',
  'node_modules/@xterm/xterm/src/browser/services/SelectionService.ts',
  'src/renderer/terminal-surface.ts',
  'crates/shellfox-desktop/src/app/links.rs',
  'crates/shellfox-desktop/src/renderer/surface.rs',
  'crates/ghostty-vt-sys/adapter.c',
  'crates/ghostty-vt-sys/experimental-native.c',
  'crates/ghostty-vt-sys/src/lib.rs',
  'crates/shellfox-mobile/src/frame.rs',
];
export async function collectDomEvidence() {
  const sourceHashes = await Promise.all(sources.map(async path => ({
    path, sha256: createHash('sha256').update(await readFile(path)).digest('hex'),
  })));
  const script = await readFile('node_modules/@xterm/xterm/lib/xterm.js', 'utf8');
  const css = await readFile('node_modules/@xterm/xterm/css/xterm.css', 'utf8');
  const browser = await chromium.launch({ headless: true });
  const cases = [];
  try {
    for (const fixture of fixtures) {
      const page = await browser.newPage({ viewport: { width: 1000, height: 300 } });
      try {
        await page.setContent('<div id="terminal"></div>');
        await page.addStyleTag({ content: css });
        await page.addScriptTag({ content: script });
        const result = await page.evaluate(async fixture => {
          // Browser-only read of the unmodified package bundle. No model/renderer replacement.
          const terminal = new window.Terminal({
            cols: fixture.cols, rows: 3, allowProposedApi: true, fontSize: 14,
            fontFamily: "'Cascadia Code', Consolas, 'DejaVu Sans Mono', monospace",
            cursorBlink: false, minimumContrastRatio: 1, drawBoldTextInBrightColors: false,
          });
          terminal.open(document.getElementById('terminal'));
          terminal.focus();
          const write = data => new Promise(resolve => terminal.write(data, resolve));
          for (const step of fixture.steps) {
            if (step.text !== undefined) await write(step.text);
            else terminal.resize(step.resize, 3);
          }
          const raw = terminal._core._bufferService.buffer.lines.get(0);
          if (fixture.synthetic) {
            const cell = raw.loadCell(0, terminal.buffer.active.getNullCell());
            if (fixture.synthetic === 'zero-stub') {
              cell.bg = 0x01000001; // palette-red, no extended attrs
              raw.setCellFromCodepoint(0, 0, 0, cell);
            } else {
              const tail = raw.loadCell(1, terminal.buffer.active.getNullCell());
              tail.bg = 0x01000001;
              raw.setCell(1, tail);
            }
          }
          // Keep the cursor on another row so it cannot extend row0 or split its spans.
          await write(fixture.cursorX === undefined ? '\x1b[2;1H' : `\x1b[1;${fixture.cursorX + 1}H`);
          await new Promise(resolve => {
            const event = terminal.onRender(() => { event.dispose(); resolve(); });
            terminal.refresh(0, terminal.rows - 1);
          });
          const cellWidth = terminal._core._renderService.dimensions.css.cell.width;
          const row = document.querySelector('.xterm-rows > div');
          const bounds = row.getBoundingClientRect();
          const spans = [...row.children].map(span => {
            const rect = span.getBoundingClientRect();
            return {
              text: span.textContent, classes: span.className,
              background: getComputedStyle(span).backgroundColor,
              left: (rect.left - bounds.left) / cellWidth, columns: rect.width / cellWidth,
              letterSpacing: span.style.letterSpacing,
            };
          });
          const cells = Array.from({ length: raw.length }, (_, x) => {
            const cell = terminal.buffer.active.getLine(0).getCell(x);
            return { x, text: cell.getChars(), semantic: cell.getWidth(), bg: raw.getBg(x) >>> 0 };
          });
          const links = [];
          for (const provider of terminal._core._linkProviderService.linkProviders) {
            await new Promise(resolve => provider.provideLinks(1, found => {
              for (const link of found || []) links.push({ text: link.text, range: link.range });
              resolve();
            }));
          }
          const screen = terminal._core.screenElement;
          const screenBounds = screen.getBoundingClientRect();
          const probes = [0.25, 0.75, 1.25, 7.25].map(paintColumn => {
            const event = new MouseEvent('mousemove', {
              clientX: screenBounds.left + paintColumn * cellWidth,
              clientY: screenBounds.top + terminal._core._renderService.dimensions.css.cell.height / 4,
            });
            const mouse = terminal._core._mouseService;
            return { paintColumn, linkGrid1: mouse.getCoords(event, screen, terminal.cols, terminal.rows),
              applicationGrid0: mouse.getMouseReportCoords(event, screen),
              selectionGrid1: mouse.getCoords(event, screen, terminal.cols, terminal.rows, true) };
          });
          const result = {
            cols: terminal.cols, backingCols: raw.length,
            noBgTrimmedLength: raw.getNoBgTrimmedLength(),
            cursor: [terminal.buffer.active.cursorX, terminal.buffer.active.cursorY],
            cursorInitialized: terminal._core.coreService.isCursorInitialized,
            cursorHidden: terminal._core.coreService.isCursorHidden, links, probes,
            unicode: terminal._core.unicodeService.activeVersion,
            rowColumns: bounds.width / cellWidth, overflow: getComputedStyle(row).overflow,
            cells, spans,
          };
          terminal.dispose();
          return result;
        }, fixture);
        cases.push({ ...fixture, ...result });
      } finally { await page.close(); }
    }
    return {
      scope: 'shipped xterm6 DOM/cell/CSS-layout evidence only; no screenshot, pixels or app acceptance',
      browser: browser.version(), sourceHashes, cases,
    };
  } finally { await browser.close(); }
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const output = process.argv[2];
  if (!output) throw new Error('usage: node dom-render-contract.mjs tmp/.../evidence.json');
  const evidence = await collectDomEvidence();
  await writeFile(output, JSON.stringify(evidence, null, 2) + '\n');
  console.log(JSON.stringify({ cases: evidence.cases.length, output, browser: evidence.browser }));
}
