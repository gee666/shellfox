import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
import { createRequire } from 'node:module';
import vm from 'node:vm';
import path from 'node:path';
const require = createRequire(import.meta.url);
const source = readFileSync('forge.config.cjs', 'utf8');
function config(platform) {
  const module = { exports: {} };
  vm.runInNewContext(source, { require, process: { platform, arch: 'x64', env: {} }, module, __dirname: path.resolve('.') });
  return module.exports;
}
test('icons exist with the expected PNG dimensions and container formats', () => {
  for (const size of [16, 24, 32, 48, 64, 128, 256, 512, 1024]) {
    const png = readFileSync(`resources/icon/${size === 1024 ? 'icon' : `icon-${size}`}.png`);
    assert.equal(png.subarray(1, 4).toString(), 'PNG');
    assert.equal(png.readUInt32BE(16), size);
    assert.equal(png.readUInt32BE(20), size);
    assert.equal(png[25], 6); // RGBA
  }
  const ico = readFileSync('resources/icon/icon.ico');
  assert.equal(ico.readUInt16LE(2), 1);
  assert.equal(ico.readUInt16LE(4), 7);
  const sizes = [];
  for (let i = 0; i < 7; i++) {
    const entry = 6 + i * 16;
    sizes.push(ico[entry] || 256);
    if (!ico[entry]) assert.equal(ico.subarray(ico.readUInt32LE(entry + 12) + 1, ico.readUInt32LE(entry + 12) + 4).toString(), 'PNG');
  }
  assert.deepEqual(sizes, [16, 24, 32, 48, 64, 128, 256]);
  const icns = readFileSync('resources/icon/icon.icns');
  assert.equal(icns.subarray(0, 4).toString(), 'icns');
  assert.equal(icns.readUInt32BE(4), icns.length);
  for (let offset = 8; offset < icns.length;) {
    const length = icns.readUInt32BE(offset + 4);
    assert.ok(length > 8 && offset + length <= icns.length);
    assert.equal(icns.subarray(offset + 9, offset + 12).toString(), 'PNG');
    offset += length;
  }
});
test('all native makers share the app icon and zip inherits the packaged icon', () => {
  for (const platform of ['win32', 'linux', 'darwin']) {
    const c = config(platform);
    assert.equal(c.packagerConfig.icon, path.resolve('resources/icon/icon'));
    assert.ok(c.makers.some(m => m.name === '@electron-forge/maker-zip'));
    assert.ok(!c.packagerConfig.ignore('/tmp/build/icon/icon.ico'));
  }
  const squirrel = config('win32').makers.find(m => m.name.endsWith('maker-squirrel')).config;
  assert.ok(existsSync(squirrel.setupIcon));
  assert.ok(squirrel.iconUrl.endsWith('/resources/icon/icon.ico'));
  const deb = config('linux').makers.find(m => m.name.endsWith('maker-deb')).config.options;
  assert.ok(existsSync(deb.icon));
  assert.match(readFileSync(deb.desktopTemplate, 'utf8'), /^Icon=shellfox$/m);
  assert.match(readFileSync('scripts/build.mjs', 'utf8'), /copyFileSync.*resources\/icon/);
  assert.match(readFileSync('src/main/index.ts', 'utf8'), /icon: path.resolve\(__dirname, '\.\.\/icon'/);
});
