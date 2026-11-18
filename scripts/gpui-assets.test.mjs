import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { test } from 'node:test';
const root = fileURLToPath(new URL('../', import.meta.url));
const read = relative => readFileSync(path.join(root, relative), 'utf8');

test('native fox is the existing Shellfox artwork, not a replacement illustration', () => {
  const source = read('src/renderer/TerminalPlaceholder.tsx').split('const FOX = `')[1].split('`;')[0];
  const native = read('crates/shellfox-desktop/src/brand.rs').split('const FOX: &str = r#"')[1].split('"#;')[0];
  assert.equal(native, source);
});

test('native app icons keep the existing SVG paths and stroke geometry', () => {
  const source = read('src/renderer/components.tsx');
  const paths = new Map();
  const expression = /(?:[ {,])(?:'([^']+)'|([a-z-]+)):\s*'([^']+)'/g;
  for (const match of source.matchAll(expression)) paths.set(match[1] ?? match[2], match[3]);
  for (const name of ['plus', 'settings', 'close', 'chevron', 'eye', 'eye-off', 'folder', 'folder-terminal', 'pin', 'copy', 'settle', 'unsettle', 'monitor', 'server', 'more', 'edit', 'connect', 'disconnect', 'check', 'shield']) {
    const svg = read(`crates/shellfox-desktop/assets/icons/${name}.svg`);
    assert.ok(svg.includes(`<path d="${paths.get(name)}"`), name);
    assert.ok(svg.includes('viewBox="0 0 16 16"'), name);
    assert.ok(svg.includes('stroke-width="1.25"'), name);
    assert.ok(svg.includes('stroke-linecap="round"'), name);
    assert.ok(svg.includes('stroke-linejoin="round"'), name);
  }
});
