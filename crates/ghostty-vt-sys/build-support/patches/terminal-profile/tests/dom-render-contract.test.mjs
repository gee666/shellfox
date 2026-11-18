import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import test from 'node:test';
import { collectDomEvidence } from '../tools/dom-render-contract.mjs';
const root = 'crates/ghostty-vt-sys/build-support/patches/terminal-profile/prototype/render-contract/';
const near = (actual, expected) => assert.ok(Math.abs(actual - expected) < 0.06, `${actual} != ${expected} CSS cell units`);

test('review kind matrix keeps storage, owner advance, semantic flow and kind separate', async () => {
  const spec = JSON.parse(await readFile(root + 'cell-kinds.json', 'utf8'));
  assert.equal(spec.status, 'review-only-proposal-not-an-exported-ABI');
  assert.equal(spec.grid_slot_stride, 1);
  assert.equal(spec.kinds.length, 8);
  const kinds = Object.fromEntries(spec.kinds.map(k => [k.kind, k]));
  assert.equal(kinds.wide_base.physical_advance, 2);
  assert.equal(kinds.orphan_wide.physical_advance, 1);
  assert.equal(kinds.orphan_wide.semantic_columns, 2);
  for (const name of ['wide_tail', 'zero_text', 'zero_stub']) {
    assert.equal(kinds[name].semantic_columns, 0);
    assert.equal(kinds[name].emits_inline_span, false);
  }
  assert.notEqual(kinds.wide_tail.native_wide, kinds.zero_stub.native_wide);
  assert.equal(kinds.wrap_padding.semantic_columns, 1);
});

test('shipped DOM renderer preserves separate paint and logical-grid contracts', { timeout: 90000 }, async t => {
  const actual = await collectDomEvidence();
  const pinned = JSON.parse(await readFile(root + 'evidence.json', 'utf8'));
  for (const source of pinned.sourceHashes.filter(s => s.path.startsWith('node_modules/@xterm/'))) {
    assert.equal(createHash('sha256').update(await readFile(source.path)).digest('hex'), source.sha256, source.path);
  }
  assert.equal(actual.cases.length, 17);
  const byName = Object.fromEntries(actual.cases.map(c => [c.name, c]));
  for (const c of actual.cases) {
    assert.equal(c.unicode, '6');
    assert.equal(c.overflow, 'hidden');
    near(c.rowColumns, c.cols);
  }
  await t.test('zero text and empty stub skip their background and collapse flow', () => {
    for (const name of ['zero-text-leading-background', 'empty-zero-stub-background']) {
      const c = byName[name];
      assert.equal(c.cells[0].semantic, 0);
      assert.equal(c.spans.length, 1);
      assert.equal(c.spans[0].text, 'A');
      assert.ok(!c.spans[0].classes.includes('xterm-bg-1'));
      near(c.spans[0].left, 0);
    }
    const middle = byName['zero-text-middle-background'];
    assert.deepEqual(middle.spans.map(s => s.text), ['A', 'B']);
    near(middle.spans[1].left, 1);
  });
  await t.test('wide tail background is skipped; normal pair advances two', () => {
    const c = byName['wide-tail-independent-background'];
    assert.deepEqual(c.cells.slice(0, 3).map(c => c.semantic), [2, 0, 1]);
    assert.deepEqual(c.spans.map(s => s.text), ['中', 'B']);
    assert.ok(c.spans[0].classes.includes('xterm-bg-4'));
    assert.ok(!c.spans.some(s => s.classes.includes('xterm-bg-1')));
    near(c.spans[1].left, 2);
  });
  await t.test('orphan owns two paint columns but one slot; following background/glyph follows flow', () => {
    const bg = byName['orphan-following-background-clipped'];
    assert.equal(bg.cells[4].text, '中'); assert.equal(bg.cells[4].semantic, 2);
    assert.equal(bg.cells[5].semantic, 1);
    near(bg.spans.find(s => s.classes.includes('xterm-bg-2')).left, 6);
    const clipped = byName['orphan-following-glyph-clipped'];
    assert.equal(clipped.cells[7].text, 'Q');
    const q = clipped.spans.find(s => s.text === 'Q'); near(q.left, 8);
    assert.deepEqual(clipped.cursor, [7, 0]); assert.match(q.classes, /xterm-cursor/);
    const grown = byName['orphan-following-glyph-regrow'];
    assert.equal(grown.cells[7].text, 'Q'); near(grown.spans.find(s => s.text === 'Q').left, 8);
    assert.equal(grown.cols, 10);
  });
  await t.test('width-zero skips cursor class without changing logical cursor', () => {
    const plain = byName['cursor-narrow'], zero = byName['cursor-zero-text-skips-class'];
    assert.equal(plain.cursorInitialized, true); assert.equal(zero.cursorHidden, false);
    assert.deepEqual(zero.cursor, [0, 0]);
    assert.ok(plain.spans.some(s => /xterm-cursor/.test(s.classes)));
    for (const name of ['cursor-zero-text-skips-class', 'cursor-wide-tail-skips-class', 'cursor-empty-zero-stub-skips-class']) {
      const c = byName[name];
      assert.equal(c.cursorInitialized, true); assert.equal(c.cursorHidden, false);
      assert.equal(c.cells[c.cursor[0]].semantic, 0);
      assert.ok(c.spans.every(s => !/xterm-cursor/.test(s.classes)), name);
    }
  });
  await t.test('hidden backing cells can enter paint after flow collapse', () => {
    const c = byName['hidden-backing-enters-paint-after-zero'];
    assert.equal(c.cols, 4); assert.equal(c.backingCols, 8);
    assert.equal(c.cells[4].text, 'D');
    assert.ok(c.noBgTrimmedLength > c.cols);
    near(c.spans.find(s => s.text === 'DEF').left, 3);
  });
  await t.test('hyperlink hit and PTY/selection grid are not painted span coordinates', () => {
    const c = byName['zero-link-source-grid-versus-paint'];
    assert.deepEqual(c.links[0].range, { start: { x: 2, y: 1 }, end: { x: 2, y: 1 } });
    near(c.spans[0].left, 0); assert.match(c.spans[0].classes, /xterm-underline/);
    assert.deepEqual(c.probes[0].linkGrid1, [1, 1]);
    assert.equal(c.probes[0].applicationGrid0.col, 0);
    assert.deepEqual(c.probes[1].selectionGrid1, [2, 1]);
    assert.equal(c.probes[1].applicationGrid0.col, 0);
    assert.deepEqual(c.probes[2].linkGrid1, [2, 1]);
  });
});
