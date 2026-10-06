// Real backend plus native modules loaded from the packaged ASAR. No test factory.
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { mkdirSync } from 'node:fs';
import { build } from 'esbuild';
import { run, root } from './common.mjs';
const [asarArgument, executableArgument] = process.argv.slice(2);
if (!asarArgument || !executableArgument) throw new Error('Expected packaged ASAR/executable paths.');
const asar = path.resolve(asarArgument), executable = path.resolve(executableArgument);
const cwd = path.join(root, 'tmp/packaged-backend-smoke', randomUUID());
mkdirSync(cwd, { recursive: true });
const output = path.join(root, 'tmp/build-cache/packaged-backend-smoke.cjs');
await build({
  stdin: { resolveDir: root, loader: 'ts', sourcefile: 'packaged-backend-smoke.ts', contents: String.raw`
import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { stripVTControlCharacters } from 'node:util';
import { createRequire } from 'node:module';
import { PtyBackend } from './src/main/terminal/backend';
import { discoverProfiles } from './src/main/terminal/profiles';
const addonPath = createRequire(__filename).resolve('node-pty').replaceAll('\\', '/').replace('app.asar.unpacked', 'app.asar');
assert.ok(addonPath.startsWith(process.argv[4].replaceAll('\\', '/') + '/node_modules/node-pty/'), 'Native factory must resolve node-pty from this package, not dev node_modules.');
const value = r => { if (!r.ok) throw Error(r.error.code + ': ' + r.error.message); return r.value; };
const backend = new PtyBackend({discover: () => discoverProfiles({supervisorOptions:{packaged:true,resourcesPath:process.resourcesPath,projectRoot:process.argv[3]}})});
const waitFor = async condition => {
  const deadline = Date.now() + 20000;
  while (!condition()) { if(Date.now()>deadline) throw Error('Packaged backend output/exit deadline'); await new Promise(r=>setTimeout(r,25)); }
};
void (async () => {
  let failure;
  try {
    const profiles = value(await backend.initialize());
    const profile = profiles.profiles.find(p => p.environment === 'local' && p.available && p.canTerminateDescendants);
    assert.ok(profile, 'No preflighted local safe-close profile.');
    const identity = {tabId:randomUUID(),generation:randomUUID()}, sessionId = randomUUID();
    let output = '';
    backend.subscribe(event => { if(event.type==='data' && event.tabId===identity.tabId) output=(output+event.data).slice(-256*1024); });
    value(await backend.launch({...identity,sessionId,profileId:profile.id,cwd:process.argv[2]}));
    value(backend.resize({...identity,cols:100,rows:40}));
    const marker = 'PACKAGED_BACKEND_' + randomUUID().replaceAll('-','');
    const command = process.platform==='win32'
      ? "Write-Output '" + marker + "'; Write-Output ('SIZE:{0}:{1}' -f $Host.UI.RawUI.WindowSize.Width,$Host.UI.RawUI.WindowSize.Height)\r"
      : "printf '%s\\n' '" + marker + "'; stty size\r";
    value(backend.write({...identity,data:command}));
    await waitFor(() => new RegExp('(?:^|[\\r\\n])'+marker+'(?:[\\r\\n]|$)').test(stripVTControlCharacters(output)));
    await waitFor(() => (process.platform==='win32' ? /(?:^|[\r\n])SIZE:100:40(?:[\r\n]|$)/ : /(?:^|[\r\n])40 100(?:[\r\n]|$)/).test(stripVTControlCharacters(output)));
    const replay = value(backend.attach({tabId:identity.tabId}));
    assert.equal(replay.generation,identity.generation); assert.ok(replay.chunks.length);
    value(backend.write({...identity,data:'exit 7\r'}));
    await waitFor(() => backend.get(identity.tabId)?.state==='closed');
    assert.equal(backend.get(identity.tabId)?.exitCode,7);
    value(await backend.close(identity));
    const second = {tabId:randomUUID(),generation:randomUUID()};
    value(await backend.launch({...second,sessionId,profileId:profile.id,cwd:process.argv[2]}));
    value(await backend.close(second));
    assert.equal(backend.get(second.tabId)?.state,'closed');
  } catch(error) { failure=error; }
  try { await backend.dispose(); } catch(error) { failure ??=error; }
  if(failure) throw failure;
  console.log('Packaged real backend/native addon passed: profiles, spawn, VT output, resize, replay, exit 7 and confirmed close.');
  process.exit(0);
})().catch(error => { console.error(error); process.exit(1); });
` },
  outfile: output, bundle: true, platform: 'node', format: 'cjs', target: 'node24',
  external: ['electron', 'node-pty', 'better-sqlite3'],
  // The production factory resolves from main's __filename. Preserve that
  // resolution base in this external fixture so dev node_modules cannot satisfy it.
  define: { __filename: JSON.stringify(path.join(asar, 'tmp/build/main/index.cjs')) },
});
run(executable, [output, cwd, root, asar], 90000, { ELECTRON_RUN_AS_NODE: '1' });
