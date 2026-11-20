import assert from 'node:assert/strict';
import { copyFile, mkdir, truncate, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import path from 'node:path';

export const original = 'P4 editor first line\n' + 'Long wrapped source '.repeat(24) + '\n';
export const updated = 'P4 editor updated\n';
export const marker = 'OPAQUE PREVIEW READY';
export const assetColor = [65, 19, 211];

export async function startContainmentProbe() {
  const requests = [];
  const server = createServer((req, res) => {
    requests.push(req.url);
    res.writeHead(200, { 'Content-Type': 'text/plain' }).end('loopback fixture');
  });
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const url = `http://127.0.0.1:${server.address().port}`;
  try {
    const control = await fetch(`${url}/control`, { signal: AbortSignal.timeout(5_000) });
    assert.equal(await control.text(), 'loopback fixture');
    assert.deepEqual(requests.splice(0), ['/control']);
  } catch (error) {
    server.close();
    throw error;
  }
  return { url, requests, close: () => new Promise(resolve => server.close(resolve)) };
}

export async function createFiles(parent, url) {
  const project = path.join(parent, 'project');
  for (const dir of [project, path.join(project, 'gone'), path.join(project, 'unique-folder')]) {
    await mkdir(dir, { recursive: true, mode: 0o700 });
  }
  const asset = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="12"><rect width="24" height="12" fill="rgb(${assetColor.join(',')})"/></svg>`;
  await Promise.all([
    writeFile(path.join(parent, 'outside.svg'), asset),
    writeFile(path.join(project, 'asset.svg'), asset),
    writeFile(path.join(project, 'notes.txt'), `\uFEFF${original}`),
    writeFile(path.join(project, 'unique-folder', 'sentinel.txt'), 'P4 folder sentinel\n'),
    writeFile(path.join(project, 'preview.md'), '# P4 Markdown heading\n\n[Open notes](notes.txt)\n'),
    writeFile(path.join(project, 'remote.svg'), `<svg xmlns="http://www.w3.org/2000/svg" width="120" height="35"><rect width="120" height="35" fill="green"/><image href="${url}/svg" width="120" height="35"/></svg>`),
    writeFile(path.join(project, 'theme.css'), '@import "colors.css";'),
    writeFile(path.join(project, 'colors.css'), 'h1 { color: rgb(20, 120, 40); font: bold 24px sans-serif; }'),
    writeFile(path.join(project, 'demo.html'), `<!doctype html><meta charset="utf-8"><title>P4 HTML fixture</title>
<link rel="stylesheet" href="theme.css">
<h1 id="status">Preview did not run</h1>
<img alt="Local asset" id="asset" src="asset.svg"><img src="missing.svg"><img src="../outside.svg">
<a id="probe" href="${url}/link">Containment probe</a>
<script src="demo.js"></script>`),
    writeFile(path.join(project, 'demo.js'), `addEventListener('load', () => {
  let denied = false;
  try { void parent.document.body; } catch { denied = true; }
  if (self.origin === 'null' && denied && typeof window.shellfox === 'undefined' &&
      document.querySelector('#asset').naturalWidth === 24) {
    document.querySelector('#status').textContent = ${JSON.stringify(marker)};
  }
  fetch('${url}/fetch').catch(() => {});
  const image = new Image();
  image.src = '${url}/image';
  document.querySelector('#probe').addEventListener('click', () => {
    window.open('${url}/popup');
    location.href = '${url}/frame';
  });
});`),
    writeFile(path.join(project, 'oversize.txt'), Buffer.alloc(8 * 1024 * 1024 + 1, 'x')),
  ]);
  const largeImage = path.join(project, 'oversize.png');
  await copyFile(new URL('../resources/icon/icon-16.png', import.meta.url), largeImage);
  await truncate(largeImage, 64 * 1024 * 1024 + 1);
  return project;
}
