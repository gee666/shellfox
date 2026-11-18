import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import path from 'node:path';
import { test } from 'node:test';
import { build } from 'esbuild';

const require = createRequire(import.meta.url);
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
async function resolver(expected, localAppData) {
  const result = await build({ bundle: true, platform: 'node', format: 'cjs', write: false,
    stdin: { contents: "export { hostNetworkHelper } from './src/host/paths';", resolveDir: process.cwd() },
    define: { __TAILSCALE_HELPER_SHA256__: JSON.stringify(expected) },
  });
  const module = { exports: {} };
  const processFixture = { platform: 'win32', arch: 'x64', versions: {}, env: { LOCALAPPDATA: localAppData }, cwd: () => process.cwd() };
  new Function('require', 'module', 'exports', 'process', result.outputFiles[0].text)(require, module, module.exports, processFixture);
  return module.exports.hostNetworkHelper;
}

test('compiled build pins enforce the same Windows network path across packaged versions without executing helpers', async () => {
  await mkdir('tmp', { recursive: true });
  const directory = await mkdtemp(path.resolve('tmp/windows-helper-anchor-unit-'));
  try {
    const localAppData = path.join(directory, 'local');
    let previous;
    for (const [version, bytes] of [['A', 'fixture build A'], ['B', 'fixture build B']]) {
      const resourcesPath = path.join(directory, 'app-' + version, 'resources');
      await mkdir(path.join(resourcesPath, 'tailscale'), { recursive: true });
      await writeFile(path.join(resourcesPath, 'tailscale/shellfox-tailscale.exe'), bytes);
      const hostNetworkHelper = await resolver(sha256(bytes), localAppData);
      const launch = await hostNetworkHelper({ resourcesPath, packaged: true });
      assert.deepEqual(launch.args, ['--expected-sha256', sha256(bytes)]);
      assert.equal(launch.executable, path.join(localAppData, 'Shellfox/helpers/win32-x64/shellfox-tailscale.exe'));
      if (previous) assert.equal(launch.executable, previous);
      previous = launch.executable;
      assert.equal(await readFile(launch.executable, 'utf8'), bytes);
      if (version === 'B') {
        await writeFile(path.join(resourcesPath, 'tailscale/shellfox-tailscale.exe'), 'tampered');
        // Matching cache bytes are reusable; a missing cache must reject the tampered candidate.
        assert.deepEqual(await hostNetworkHelper({ resourcesPath, packaged: true }), launch);
        await rm(launch.executable);
        await assert.rejects(hostNetworkHelper({ resourcesPath, packaged: true }), /SHA256 mismatch/);
      }
    }
  } finally { await rm(directory, { recursive: true, force: true }); }
});

test('Electron, desktop CLI and headless compile the freshly linked helper digest, not a runtime target hash', async () => {
  const helper = await readFile('scripts/tailscale.mjs', 'utf8');
  assert.match(helper, /sha256 = createHash\('sha256'\)\.update\(readFileSync\(partial\)\)\.digest\('hex'\)/);
  assert.ok(helper.indexOf('sha256 = createHash') < helper.indexOf('renameSync(partial, executable)'));
  for (const file of ['scripts/build.mjs', 'scripts/headless-build.mjs', 'scripts/shellfox-cli-build.mjs']) {
    const source = await readFile(file, 'utf8');
    assert.ok(source.includes('buildHelperWithAnchor'));
    assert.ok(source.includes('__TAILSCALE_HELPER_SHA256__: JSON.stringify('));
  }
  const desktop = await readFile('scripts/build.mjs', 'utf8');
  assert.ok(desktop.includes("await buildShellfoxCli(output + '/cli', helperBuild.sha256)"));
});
