import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, open, readFile, readdir, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { eventually } from './macos-p4-dialog.mjs';

export const transferBytes = 393_233;
const transferName = 'p4-fixture.bin';
const cancelName = 'cancel-p4.bin';
const stage = /^\.shellfox-(transfer|download)-/;

const read = file => readFile(file).catch(error => {
  if (error.code === 'ENOENT') return null;
  throw error;
});

export async function exerciseTransfers({ parent, home, remote, picker, openUpload, openDownload, selectRemote, cancelUpload, idle, shot,
  chooseCancelProbe = async () => { await picker.wait(); await picker.choose(cancelName); },
}) {
  const upload = path.join(parent, 'upload');
  const download = path.join(home, 'Downloads');
  await Promise.all([upload, download].map(dir => mkdir(dir, { recursive: true, mode: 0o700 })));
  const payload = Buffer.alloc(transferBytes, 73);
  payload[0] = 0;
  payload[payload.length - 1] = 255;
  await writeFile(path.join(upload, transferName), payload);
  const target = path.join(remote, transferName);
  const destination = path.join(download, transferName);
  const untouched = Buffer.from('keep this file unless Replace is accepted\n');
  await writeFile(destination, untouched);

  await openUpload();
  const uploadDialog = await picker.wait();
  await picker.cancel();
  await idle();
  assert.equal(await read(target), null, 'Cancelling the native upload picker must not upload');
  await openUpload();
  await picker.wait();
  await picker.choose(transferName);
  await eventually('host received the exact multi-chunk upload', () => read(target), bytes => bytes?.equals(payload));
  await idle();
  await shot('21-peer-file-uploaded');

  await selectRemote(transferName);
  await openDownload();
  const downloadDialog = await picker.wait();
  await picker.cancel();
  await idle();
  assert.deepEqual(await readFile(destination), untouched);
  await openDownload();
  await picker.wait();
  await picker.save(transferName);
  await picker.replace(false);
  await idle();
  assert.deepEqual(await readFile(destination), untouched, 'Declining the native overwrite must preserve local bytes');
  await openDownload();
  await picker.wait();
  await picker.save(transferName);
  await picker.replace(true);
  await eventually('download committed exact bytes to the chosen local file', () => read(destination), bytes => bytes?.equals(payload));
  await idle();
  await shot('22-peer-file-downloaded');

  const large = await open(path.join(upload, cancelName), 'wx');
  try { await large.truncate(48 * 1024 * 1024); } finally { await large.close(); }
  await openUpload();
  await chooseCancelProbe();
  const partials = () => readdir(remote).then(names => names.filter(name => stage.test(name)));
  await eventually('an active host upload stage contains bytes', async () => {
    const stages = await partials();
    if (stages.length !== 1) return null;
    return stat(path.join(remote, stages[0])).catch(error => {
      if (error.code === 'ENOENT') return null;
      throw error;
    });
  }, status => status?.size > 0);
  await cancelUpload();
  await idle();
  await eventually('aborted upload removed its owned stage', partials, names => names.length === 0);
  assert.equal(await read(path.join(remote, cancelName)), null, 'An aborted upload must not publish');
  assert.deepEqual((await readdir(download)).filter(name => stage.test(name)), []);
  await shot('23-peer-upload-cancelled');
  await writeFile(path.join(parent, 'native-transfers.json'), JSON.stringify({ bytes: payload.length,
    sha256: createHash('sha256').update(payload).digest('hex'), uploadDialog, downloadDialog, stagingFiles: [] }, null, 2));
}
