import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, utimesSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { test } from 'node:test';
import { ensureDevelopmentHelper } from './tailscale.mjs';

test('dev launch rebuilds a missing, old or incompatible helper', t => {
  const directory = mkdtempSync(path.resolve('tmp/gpui-helper-test-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const sourceDirectory = path.join(directory, 'src');
  mkdirSync(sourceDirectory);
  const source = path.join(sourceDirectory, 'helper.go');
  writeFileSync(source, 'package main');
  const target = { directory, basename: 'helper' };
  const executable = path.join(directory, target.basename);
  const built = { executable, sha256: 'trusted build hash' };
  let builds = 0;
  let compatible = true;
  const prepare = () => ensureDevelopmentHelper({
    target, sourceDirectory,
    probe: file => {
      assert.equal(file, executable);
      if (!compatible) throw new Error('expected 2, found 1');
    },
    build: () => { builds++; return built; },
  });
  assert.equal(prepare(), built);
  writeFileSync(executable, 'binary');
  utimesSync(executable, new Date(1000), new Date(1000));
  assert.equal(prepare(), built);
  utimesSync(source, new Date(0), new Date(0));
  assert.match(prepare().sha256, /^[a-f0-9]{64}$/);
  compatible = false;
  assert.equal(prepare(), built);
  assert.equal(builds, 3);
});
