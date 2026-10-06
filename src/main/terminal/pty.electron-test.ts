// Real addon smoke, run in Electron after the packaging owner installs/rebuilds node-pty.
import { app } from 'electron';
import assert from 'node:assert/strict';
import path from 'node:path';
import { mkdir } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import { stripVTControlCharacters } from 'node:util';
import { PtyBackend } from './backend';
import type { Result, TerminalEvent } from '../../shared/contracts';
declare const __PROJECT_ROOT__: string;
const value = <T>(r: Result<T>): T => { if (!r.ok) throw new Error(r.error.code + ': ' + r.error.message); return r.value; };
async function waitFor(condition: () => boolean, timeout = 10000): Promise<void> {
  const start = Date.now(); while (!condition()) { if (Date.now() - start > timeout) throw new Error('Owned PTY smoke timed out.'); await new Promise(resolve => setTimeout(resolve, 25)); }
}
async function run() {
  const cwd = path.join(__PROJECT_ROOT__, 'tmp', 'native-pty-smoke', randomUUID()); await mkdir(cwd, { recursive: true }); app.setPath('userData', cwd); await app.whenReady();
  const backend = new PtyBackend();
  try {
    const discovered = value(await backend.initialize()), profile = discovered.profiles.find(p => p.environment === 'local' && p.available); assert.ok(profile, 'No local native shell available.');
    const identity = { tabId: randomUUID(), generation: randomUUID() }, sessionId = randomUUID();
    const events: TerminalEvent[] = []; let output = '';
    backend.subscribe(event => { events.push(event); if (event.type === 'data' && event.tabId === identity.tabId) output = (output + event.data).slice(-1024 * 1024); });
    value(await backend.launch({ ...identity, sessionId, profileId: profile.id, cwd }));
    value(backend.resize({ ...identity, cols: 100, rows: 40 }));
    const marker = 'PTY_SMOKE_' + randomUUID().replaceAll('-', '');
    value(backend.write({ ...identity, data: process.platform === 'win32' ? `Write-Output '${marker}'; Write-Output ('SIZE:{0}:{1}' -f $Host.UI.RawUI.WindowSize.Width,$Host.UI.RawUI.WindowSize.Height)\r` : `printf '%s\\n' '${marker}'; stty size\r` }));
    await waitFor(() => new RegExp('(?:^|[\\r\\n])' + marker + '(?:[\\r\\n]|$)').test(stripVTControlCharacters(output)));
    await waitFor(() => process.platform === 'win32' ? /(?:^|[\r\n])SIZE:100:40(?:[\r\n]|$)/.test(stripVTControlCharacters(output)) : /(?:^|[\r\n])40 100(?:[\r\n]|$)/.test(stripVTControlCharacters(output)));
    const replay = value(backend.attach({ tabId: identity.tabId })); assert.ok(replay.chunks.length); assert.equal(replay.generation, identity.generation); assert.ok(replay.chunks.every((c, i) => !i || c.sequence > replay.chunks[i - 1].sequence));
    value(backend.write({ ...identity, data: 'exit 7\r' })); await waitFor(() => backend.get(identity.tabId)?.state === 'closed'); assert.equal(backend.get(identity.tabId)?.exitCode, 7);
    const second = { tabId: randomUUID(), generation: randomUUID() }; value(await backend.launch({ ...second, sessionId, profileId: profile.id, cwd })); value(await backend.close(second)); assert.equal(backend.get(second.tabId)?.state, 'closed');
    assert.ok(events.some(e => e.type === 'exit' && e.tabId === identity.tabId));
    console.log('Real Electron PTY smoke passed: spawn, VT output, resize, replay, shell exit code, explicit close.');
  } finally { await backend.dispose(); }
  app.exit(0);
}
void run().catch(error => { console.error('Real Electron PTY smoke failed: ' + String(error)); app.exit(1); });
