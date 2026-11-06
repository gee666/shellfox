// Host-side preflight of the same production RemoteClient/RemoteHost used by Android QA.
// This is not Android UI coverage. The Android run must still render and edit these real host files.
import { build } from 'esbuild';
import { createRequire } from 'node:module';
import { createHash, createHmac, randomUUID } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import assert from 'node:assert/strict';
import { startRemoteHost } from './start-remote-host.mjs';
const host = await startRemoteHost();
let client;
try {
  await build({ entryPoints: ['mobile/src/client/client.ts', 'src/shared/file-api.ts'], bundle: true, platform: 'node', format: 'cjs', outdir: path.join(host.runDirectory, 'client'), outbase: '.', external: [] });
  const require = createRequire(import.meta.url), { RemoteClient } = require(path.join(host.runDirectory, 'client/mobile/src/client/client.js'));
  const { uploadFileChunks, downloadFileChunks } = require(path.join(host.runDirectory, 'client/src/shared/file-api.js'));
  const config = host.ready.configuration;
  const peer = Buffer.from(JSON.stringify(config.clientPeer)).toString('base64url');
  const transport = { request: async (peerId, route, body) => {
    assert.equal(peerId, config.hostId);
    const timestamp = String(Date.now()), message = [timestamp, peer, 'POST', route, createHash('sha256').update(body).digest('hex')].join('\n');
    const signature = createHmac('sha256', config.hmacSecret).update(message).digest('hex');
    const response = await fetch(host.ready.baseUrl + route, { method: 'POST', headers: { 'Content-Type': 'application/json', 'x-shellfox-peer': peer, 'x-shellfox-timestamp': timestamp, 'x-shellfox-signature': signature }, body, signal: AbortSignal.timeout(35000) });
    return response.text();
  } };
  client = new RemoteClient(config.hostId, transport, { createClientId: randomUUID, approvalRetryMs: 100, reconnectDelayMs: 100 });
  async function waitFor(check) { const deadline = Date.now() + 10000; while (Date.now() < deadline) { if (await check()) return; await new Promise(resolve => setTimeout(resolve, 25)); } throw new Error('QA client state timeout: ' + JSON.stringify(client.getState())); }
  function value(result) { assert.equal(result.ok, true, JSON.stringify(result)); return result.value; }
  client.connect(); await waitFor(() => client.getState().status === 'awaiting-approval');
  assert.equal((await client.files.list({ path: config.root })).ok, false);
  await host.control('approve'); await waitFor(() => client.getState().status === 'connected');
  const listing = value(await client.files.list({ path: config.root })); assert(listing.entries.some(entry => entry.name === 'code.ts'));
  const codePath = path.join(config.root, 'code.ts'), code = value(await client.files.read({ path: codePath })); assert.equal(code.kind, 'text');
  const changed = code.text + '// Saved through production RemoteClient to actual Windows disk.\n';
  value(await client.files.write({ path: codePath, revision: code.revision, text: changed })); assert.equal(await readFile(codePath, 'utf8'), changed);
  const image = value(await client.files.read({ path: path.join(config.root, 'image.png') })); assert.equal(image.kind, 'image');
  const assets = [];
  for (const assetPath of ['style.css', 'local.js', 'image.png']) { const asset = value(await client.files.readAsset({ path: path.join(config.root, 'index.html'), assetPath })); assert(asset.size > 0); assets.push(assetPath); }
  const upload = Buffer.alloc(700 * 1024 + 7, 113), uploadPath = path.join(config.root, 'node-preflight-upload.bin');
  value(await uploadFileChunks(client.files, { path: uploadPath, size: upload.length, mode: 'create', content: 'file' }, async (offset, length) => upload.subarray(offset, offset + length)));
  assert.equal(createHash('sha256').update(await readFile(uploadPath)).digest('hex'), createHash('sha256').update(upload).digest('hex'));
  const downloaded = createHash('sha256'); value(await downloadFileChunks(client.files, { path: path.join(config.root, 'transfer.bin') }, async bytes => { downloaded.update(bytes); }));
  const expected = host.ready.initialFiles.find(file => file.name === 'transfer.bin').sha256; assert.equal(downloaded.digest('hex'), expected);
  await host.control('revoke'); assert.equal((await client.files.read({ path: codePath })).ok, false);
  const status = await host.status();
  const proof = { coverage: 'Host-side production client/host preflight; not Android UI', hostPlatform: host.ready.hostPlatform, hostPid: host.ready.hostPid, root: config.root,
    approvalRequired: true, approvedPair: true, textSaveVerifiedOnDisk: true, imageRead: true, siblingAssets: assets, uploadShaVerified: true, downloadShaVerified: true, revokedReadDenied: true, requests: status.requests };
  await writeFile(path.join(host.runDirectory, 'client-proof.json'), JSON.stringify(proof, null, 2));
  console.log(JSON.stringify({ result: 'HOST_PREFLIGHT_PASS', runDirectory: host.runDirectory, requests: status.requests.length }));
} finally { client?.stop(); await host.stop(); }
