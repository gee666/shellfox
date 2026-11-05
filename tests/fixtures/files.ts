import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { crc32, deflateSync } from 'node:zlib';

export const original = 'first line\nsecond line\nthird line\n';
function pngFixture(width: number, height: number): Buffer {
  const chunk = (type: string, data: Buffer) => {
    const size = Buffer.alloc(4), checksum = Buffer.alloc(4), body = Buffer.concat([Buffer.from(type), data]);
    size.writeUInt32BE(data.length); checksum.writeUInt32BE(crc32(body));
    return Buffer.concat([size, body, checksum]);
  };
  const header = Buffer.alloc(13), rows = Buffer.alloc(height * (1 + width * 4));
  header.writeUInt32BE(width, 0); header.writeUInt32BE(height, 4); header[8] = 8; header[9] = 6;
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
    const offset = y * (1 + width * 4) + 1 + x * 4;
    rows.set([x * 20, y * 30, 100, 255], offset);
  }
  return Buffer.concat([Buffer.from('89504e470d0a1a0a', 'hex'), chunk('IHDR', header), chunk('IDAT', deflateSync(rows)), chunk('IEND', Buffer.alloc(0))]);
}
const pixel = pngFixture(8, 4);
export async function populate(directory: string) {
  await mkdir(path.join(directory, 'nested folder'), { recursive: true });
  await mkdir(path.join(directory, 'assets'), { recursive: true });
  await Promise.all([
    writeFile(path.join(directory, 'notes.txt'), original),
    writeFile(path.join(directory, 'nested folder', 'child.txt'), 'nested content\n'),
    writeFile(path.join(directory, 'README.md'), '---\ntitle: Hidden metadata\n---\n# Files preview\n\n**Bold text** and `inline code`.\n\n- [x] Finished\n\n![Markdown pixel](./pixel.png)\n\n[Open child](./nested%20folder/child.txt)\n\n<script>document.body.dataset.compromised="yes"</script>\n'),
    writeFile(path.join(directory, 'pixel.png'), pixel),
    writeFile(path.join(directory, 'binary.bin'), Buffer.from([0, 255, 0, 1])),
    writeFile(path.join(directory, 'assets', 'style.css'), '@import "./imported.css"; #result { color: rgb(17, 34, 51); }'),
    writeFile(path.join(directory, 'assets', 'imported.css'), '#result { border-top: 3px solid; background-image: url("./pixel.png"); }'),
    writeFile(path.join(directory, 'assets', 'app.js'), 'document.querySelector("#run").addEventListener("click", () => { document.querySelector("#result").textContent = "Script ran"; }); document.body.dataset.scriptReady = "true";'),
    writeFile(path.join(directory, 'assets', 'pixel.png'), pixel),
    writeFile(path.join(directory, 'assets', 'inside.html'), '<p>Asset confinement fixture</p>'),
    writeFile(path.join(directory, 'preview.html'), '<!doctype html><html><head><link rel="stylesheet" href="./assets/style.css"></head><body><button id="run">Run script</button><p id="result">Ready</p><img alt="Relative image" src="./assets/pixel.png"><pre id="isolation"></pre><script>document.querySelector("#isolation").textContent = JSON.stringify({ shellfox: typeof window.shellfox, remote: typeof window.shellfoxRemote, require: typeof window.require, process: typeof window.process, ipc: typeof window.ipcRenderer, parent: (() => { try { return typeof parent.shellfox; } catch { return "blocked"; } })() });</script><script src="./assets/app.js"></script></body></html>'),
  ]);
}

