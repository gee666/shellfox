// Invoke backend path resolution in packaged Electron, not development Electron.
import path from 'node:path';
import { build } from 'esbuild';
import { run, runNode, root } from './common.mjs';
if (process.platform !== 'darwin') throw new Error('Packaged helper preflight needs macOS.');
const [resources, executable] = process.argv.slice(2);
if (!resources || !executable) throw new Error('Expected packaged resources and executable paths.');
runNode('scripts/terminal-native-preflight.mjs', [path.join(resources, 'terminal-native')], 60000);
run('codesign', ['--verify', '--deep', '--strict', '--verbose=2', path.resolve(resources, '../..')], 60000);
const output = path.join(root, 'tmp/build-cache/mac-packaged-helper-preflight.cjs');
await build({
  stdin: {
    resolveDir: root, loader: 'ts', sourcefile: 'packaged-helper-preflight.ts',
    contents: `import assert from 'node:assert/strict';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { getProcessTrackingCapability, macTrackingHelperPath } from './src/main/terminal/tracking';
import { getDarwinSupervisorCapability, darwinSupervisorPath } from './src/main/terminal/darwin-supervisor';
const resourcesPath = process.argv[2];
const options = {packaged:true, resourcesPath, projectRoot:process.argv[3]};
void (async () => {
  assert.equal(macTrackingHelperPath(options), path.join(resourcesPath, 'terminal-native/shellfox-process-snapshot'));
  const tracking = await getProcessTrackingCapability(options);
  assert.equal(tracking.available, true, tracking.reason ?? 'Packaged tracking preflight failed.');
  assert.equal(darwinSupervisorPath(options), path.join(resourcesPath, 'terminal-native/shellfox-terminal-supervisor'));
  const supervisor = await getDarwinSupervisorCapability(options);
  assert.equal(supervisor.available, true, supervisor.reason ?? 'Packaged supervisor preflight failed.');
  assert.equal(supervisor.helperPath, darwinSupervisorPath(options));
  const missingOptions = {...options, resourcesPath:path.join(process.argv[3], 'tmp', 'absent-packaged-helper-resources-' + randomUUID())};
  const absent = await getProcessTrackingCapability(missingOptions);
  assert.equal(absent.available, false, 'Missing packaged snapshot must not fall back to development artifact.');
  const absentSupervisor = await getDarwinSupervisorCapability(missingOptions);
  assert.equal(absentSupervisor.available, false, 'Missing packaged supervisor must not fall back to development artifact.');
  assert.equal(absentSupervisor.helperPath, darwinSupervisorPath(missingOptions));
  console.log('Packaged backend helper path/preflight passed without development fallback.');
})().catch(error => { console.error(error); process.exitCode=1; });`,
  },
  outfile: output, bundle: true, platform: 'node', format: 'cjs',
  target: 'node24', external: ['electron', 'node-pty', 'better-sqlite3'],
});
run(executable, [output, resources, root], 60000, { ELECTRON_RUN_AS_NODE: '1' });
