// QA-only Windows process launcher. Does not rebuild native files or touch an enrolled tailnet.
import { build } from 'esbuild';
import { spawn } from 'node:child_process';
import { mkdir, readFile, open } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const project = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
export async function startRemoteHost() {
  if (process.platform !== 'win32') throw new Error('The real remote-device QA host must run on Windows.');
  const runDirectory = path.join(project, 'tmp', 'mobile-real-remote-host', `${Date.now()}-${process.pid}`);
  await mkdir(runDirectory, { recursive: true });
  const bundle = path.join(runDirectory, 'remote-host.cjs');
  await build({ entryPoints: [path.join(project, 'mobile/e2e/remote-host-driver.ts')], outfile: bundle, bundle: true, platform: 'node', format: 'cjs', target: 'node24', external: ['electron', 'node-pty', 'better-sqlite3'] });
  const environment = { ...process.env, SHELLFOX_REAL_FILES_QA: '1', TMP: path.join(project, 'tmp'), TEMP: path.join(project, 'tmp'), TMPDIR: path.join(project, 'tmp') };
  for (const key of ['ELECTRON_RUN_AS_NODE', 'TS_AUTH_KEY', 'TS_AUTHKEY', 'TS_CLIENT_SECRET', 'TS_CLIENT_ID', 'TS_ID_TOKEN']) delete environment[key];
  const output = await open(path.join(runDirectory, 'host-process.log'), 'a');
  const child = spawn(path.join(project, 'node_modules/electron/dist/electron.exe'), [bundle, '--run-dir', runDirectory], { cwd: project, env: environment, stdio: ['ignore', output.fd, output.fd], windowsHide: true });
  const exit = new Promise(resolve => { child.once('exit', (code, signal) => resolve({ code, signal })); child.once('error', error => resolve({ error })); });
  let ready;
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) {
    try { ready = JSON.parse(await readFile(path.join(runDirectory, 'ready.json'), 'utf8')); break; }
    catch (error) { if (error.code !== 'ENOENT' && !(error instanceof SyntaxError)) throw error; }
    if (child.exitCode !== null) break;
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  if (!ready) { child.kill(); await output.close(); throw new Error('Real QA host did not start. A busy port41973 is never taken over. Inspect ' + path.join(runDirectory, 'host-process.log')); }
  const control = async action => {
    const response = await fetch(ready.baseUrl + '/__qa/control', { method: 'POST', headers: { Authorization: 'Bearer ' + ready.controlToken, 'Content-Type': 'application/json' }, body: JSON.stringify({ action }) });
    if (!response.ok) throw new Error('QA control failed: ' + response.status); return response.json();
  };
  const status = async () => {
    const response = await fetch(ready.baseUrl + '/__qa/status', { headers: { Authorization: 'Bearer ' + ready.controlToken } });
    if (!response.ok) throw new Error('QA status failed: ' + response.status); return response.json();
  };
  let stopped = false;
  return { runDirectory, ready, child, control, status, stop: async () => {
    if (stopped) return; stopped = true;
    await control('shutdown').catch(() => {});
    const finished = await Promise.race([exit.then(() => true), new Promise(resolve => setTimeout(() => resolve(false), 10000))]);
    if (!finished) child.kill(); await output.close();
  } };
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const host = await startRemoteHost(); console.log(JSON.stringify({ runDirectory: host.runDirectory, ready: host.ready }));
  process.once('SIGINT', () => { void host.stop(); }); process.once('SIGTERM', () => { void host.stop(); });
}
