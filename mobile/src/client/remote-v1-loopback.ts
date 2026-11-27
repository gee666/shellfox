import assert from 'node:assert/strict';
import { createHash, createHmac, randomUUID } from 'node:crypto';
import { join } from 'node:path';
import { readFile, writeFile } from 'node:fs/promises';
import { setTimeout as delay } from 'node:timers/promises';
import type { Result } from '../../../src/shared/contracts';
import { RemoteClient } from './client';

function unwrap<T>(reply: Result<T>): T {
  if (!reply.ok) throw new Error(`${reply.error.code}: ${reply.error.message}`);
  return reply.value;
}
async function until(check: () => boolean) {
  const end = Date.now() + 15_000;
  while (!check()) {
    if (Date.now() >= end) throw new Error('Timed out waiting for mobile v1 loopback event.');
    await delay(25);
  }
}
async function main() {
  const { SHELLFOX_TEST_ADDR: address, SHELLFOX_TEST_SECRET: secret, SHELLFOX_TEST_CWD: cwd } = process.env;
  assert(address && secret && cwd);
  const peer = Buffer.from(JSON.stringify({ id: 'n-mobile-loopback', hostname: 'loopback-phone' })).toString('base64url');
  let clock = 0;
  const transport: ConstructorParameters<typeof RemoteClient>[1] = {
    async request(_peer, path, body) {
      clock = Math.max(Date.now(), clock + 1);
      const digest = createHash('sha256').update(body).digest('hex');
      const signature = createHmac('sha256', secret).update([String(clock), peer, 'POST', path, digest].join('\n')).digest('hex');
      const response = await fetch(`http://${address}${path}`, {
        method: 'POST', body, headers: {
          'X-Shellfox-Peer': peer, 'X-Shellfox-Timestamp': String(clock), 'X-Shellfox-Signature': signature,
        },
        signal: AbortSignal.timeout(24_000),
      });
      return response.text();
    },
  };
  const client = new RemoteClient('n-host', transport, { createClientId: randomUUID, approvalRetryMs: 50 });
  // A second view of the same device, as each one-shot `shellfox --host` call is.
  const oneShot = new RemoteClient('n-host', transport, { createClientId: randomUUID, approvalRetryMs: 50 });
  const received: { data: string; sequence: number }[] = [];
  const settlements: (string | null)[] = [];
  const remove = client.subscribeTerminal(event => {
    if (event.type === 'data') received.push(event);
    if (event.type === 'settlement') settlements.push(event.settledAt);
  });
  let changes = 0;
  const removeChanges = client.subscribe(event => { if (event.type === 'changed') changes++; });
  try {
    client.connect();
    await until(() => client.getState().status === 'connected');
    assert.equal(unwrap(await client.getSnapshot()).sessions.length, 0);
    const session = unwrap(await client.createSession({ requestId: randomUUID(), title: 'loopback terminal', cwd }));
    assert(unwrap(await client.getSnapshot()).sessions.some(({ id }) => id === session.id));
    const tabId = session.tabs[0]?.id;
    assert(tabId);
    const attachment = unwrap(await client.attachTerminal({ tabId }));
    assert(attachment.control === 'owned');
    const identity = { tabId, generation: attachment.generation, controlEpoch: attachment.controlEpoch };
    unwrap(await client.acknowledgeTerminal({ ...identity, sequence: attachment.lastSequence }));
    const marker = `mobile_v1_${randomUUID()}`;
    const windows = process.platform === 'win32';
    const command = session.tabs[0]?.profileId === 'cmd'
      ? `set sf=${marker.slice(0, 14)}\rset sf=%sf%${marker.slice(14)}\recho %sf%\r`
      : windows ? `'${marker.slice(0, 14)}'+'${marker.slice(14)}'\r`
        : `printf '%s%s\\n' '${marker.slice(0, 14)}' '${marker.slice(14)}'\r`;
    unwrap(await client.writeTerminal({ ...identity, data: command }));
    await until(() => received.map(event => event.data).join('').includes(marker));
    // The title the shell publishes names the automatic tab; a change event tells the phone to read it.
    const title = `Loopback ${randomUUID().slice(0, 8)}`;
    const tabTitle = async () => unwrap(await client.getSnapshot()).sessions.find(({ id }) => id === session.id)?.tabs[0]?.title;
    assert.match(await tabTitle() ?? '', /^Shell /);
    const titleCommand = session.tabs[0]?.profileId === 'cmd' ? `title ${title}\r`
      : windows ? `$Host.UI.RawUI.WindowTitle = '${title}'\r` : `printf '\\033]2;%s\\007' '${title}'\r`;
    // Read again after each change event, counted before the read so none is missed.
    let seen = changes, named = false;
    unwrap(await client.writeTerminal({ ...identity, data: titleCommand }));
    for (let attempt = 0; attempt < 60 && !named; attempt++) {
      await until(() => changes > seen);
      seen = changes;
      named = await tabTitle() === title;
    }
    assert(named, 'published title never reached the phone');
    const last = received.at(-1)?.sequence;
    assert(last !== undefined);
    unwrap(await client.acknowledgeTerminal({ ...identity, sequence: last }));
    unwrap(await client.resizeTerminal({ ...identity, cols: 93, rows: 27 }));
    unwrap(await client.settleSession({ sessionId: session.id, confirmActive: true }));
    await until(() => typeof settlements.at(-1) === 'string');
    unwrap(await client.unsettleSession({ sessionId: session.id }));
    await until(() => settlements.length >= 2 && settlements.at(-1) === null);
    // The owner taking control again keeps its epoch, as 0.9 did.
    unwrap(await client.takeTerminalControl({ tabId, generation: identity.generation }));
    const upload = unwrap(await client.beginTerminalUpload({ ...identity, fileName: 'phone photo.txt', size: 6 }));
    unwrap(await client.writeTerminalUpload({ ...identity, uploadId: upload.uploadId, offset: 0, data: Buffer.from('upload').toString('base64') }));
    const saved = unwrap(await client.finishTerminalUpload({ ...identity, uploadId: upload.uploadId }));
    assert.equal(saved.fileName, 'phone photo.txt');
    assert.equal(await readFile(saved.path, 'utf8'), 'upload');
    const dropped = unwrap(await client.beginTerminalUpload({ ...identity, fileName: 'dropped.txt', size: 3 }));
    unwrap(await client.cancelTerminalUpload({ ...identity, uploadId: dropped.uploadId }));
    unwrap(await client.detachTerminal({ tabId, generation: identity.generation }));
    const reattached = unwrap(await client.attachTerminal({ tabId }));
    assert(reattached.control === 'owned' && reattached.snapshot && reattached.truncated);
    assert.equal(reattached.generation, identity.generation);
    assert(reattached.controlEpoch > identity.controlEpoch);
    assert.equal(reattached.cols, 93);
    assert.equal(reattached.rows, 27);
    assert(reattached.chunks.map(chunk => chunk.data).join('').includes(marker));
    assert.equal((await client.writeTerminal({ ...identity, data: 'echo stale\r' })).ok, false);
    const listing = unwrap(await client.files.list({ sessionId: session.id, tabId, path: cwd }));
    assert(listing.entries.some(entry => entry.name === 'loopback.txt'));
    const read = unwrap(await client.request('readFile', { path: join(cwd, 'loopback.txt') }));
    assert(read.kind === 'text' && read.text === 'owned loopback fixture');
    const file = unwrap(await client.request('beginFileRead', { path: join(cwd, 'loopback.txt') }));
    const chunk = unwrap(await client.request('readFileChunk', { transferId: file.transferId, offset: 0 }));
    assert.equal(Buffer.from(chunk.data, 'base64').toString(), 'owned loopback fixture');
    const uploaded = Buffer.from('mobile transfer');
    const write = unwrap(await client.request('beginFileWrite', {
      sessionId: session.id, tabId, path: 'uploaded.txt', content: 'text', mode: 'create', size: uploaded.length,
    }));
    unwrap(await client.request('writeFileChunk', { transferId: write.transferId, offset: 0, data: uploaded.toString('base64') }));
    unwrap(await client.request('finishFileWrite', { transferId: write.transferId }));
    const result = unwrap(await client.request('readFile', { path: join(cwd, 'uploaded.txt') }));
    assert(result.kind === 'text' && result.text === 'mobile transfer');
    oneShot.connect();
    await until(() => oneShot.getState().status === 'connected');
    const borrowed = { tabId, generation: reattached.generation, controlEpoch: reattached.controlEpoch };
    const blocked = await oneShot.writeTerminal({ ...borrowed, data: 'echo blocked\r' });
    assert(!blocked.ok && blocked.error.code === 'TERMINAL_CONTROL_REQUIRED');
    unwrap(await client.detachTerminal({ tabId, generation: reattached.generation }));
    // Nobody controls the terminal now: the other view's write goes through and hands control back.
    const marker2 = `mobile_v1_${randomUUID()}`;
    unwrap(await oneShot.writeTerminal({ ...borrowed, data: command.replaceAll(marker.slice(0, 14), marker2.slice(0, 14)).replaceAll(marker.slice(14), marker2.slice(14)) }));
    unwrap(await oneShot.resizeTerminal({ ...borrowed, cols: 91, rows: 26 }));
    unwrap(await oneShot.acknowledgeTerminal({ ...borrowed, sequence: 0 }));
    let echoed = false;
    for (let attempt = 0; attempt < 40 && !echoed; attempt++) {
      const view = unwrap(await client.attachTerminal({ tabId }));
      assert(view.control === 'owned');
      echoed = view.chunks.map(chunk => chunk.data).join('').includes(marker2);
      unwrap(await client.detachTerminal({ tabId, generation: view.generation }));
      if (!echoed) await delay(250);
    }
    assert(echoed, 'one-shot write output missing');
    oneShot.stop();
    await writeFile(join(cwd, 'revoke-device'), '');
    await until(() => client.getState().status !== 'connected');
    assert.equal((await client.getSnapshot()).ok, false);
  } finally {
    removeChanges();
    remove();
    client.stop();
  }
}

void main().catch(error => { console.error(error); process.exitCode = 1; });
