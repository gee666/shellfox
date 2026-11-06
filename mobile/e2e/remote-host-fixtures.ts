import path from 'node:path';
import { mkdir, writeFile, readdir, stat, open } from 'node:fs/promises';
import { createReadStream } from 'node:fs';
import { createHash } from 'node:crypto';
import { crc32, deflateSync } from 'node:zlib';

export function qaImage(): Buffer {
  const width = 192, height = 96;
  const chunk = (type: string, data: Buffer) => {
    const size = Buffer.alloc(4), checksum = Buffer.alloc(4), body = Buffer.concat([Buffer.from(type), data]);
    size.writeUInt32BE(data.length); checksum.writeUInt32BE(crc32(body)); return Buffer.concat([size, body, checksum]);
  };
  const header = Buffer.alloc(13), rows = Buffer.alloc(height * (1 + width * 4));
  header.writeUInt32BE(width, 0); header.writeUInt32BE(height, 4); header[8] = 8; header[9] = 6;
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) rows.set([x * 255 / width, y * 255 / height, 180, 255], y * (1 + width * 4) + 1 + x * 4);
  return Buffer.concat([Buffer.from('89504e470d0a1a0a', 'hex'), chunk('IHDR', header), chunk('IDAT', deflateSync(rows)), chunk('IEND', Buffer.alloc(0))]);
}
export async function populateRemoteHost(root: string): Promise<void> {
  await mkdir(path.join(root, 'host-subfolder'), { recursive: true });
  const binary = Buffer.alloc(3 * 1024 * 1024 + 31); for (let i = 0; i < binary.length; i++) binary[i] = (i * 37 + 11) % 256;
  await Promise.all([
    writeFile(path.join(root, 'code.ts'), "const hostDevice = 'Actual Windows host filesystem';\nconsole.log(hostDevice);\n"),
    writeFile(path.join(root, 'README.md'), '# Windows remote device\n\nThese files live on the Windows host, outside Android.\n\n![Windows host image](image.png)\n'),
    writeFile(path.join(root, 'index.html'), '<!doctype html><html><head><link rel="stylesheet" href="style.css"></head><body><h1>Actual Windows host</h1><p id="proof">Loading sibling script</p><button id="run">Run remote JavaScript</button><img alt="Windows host sibling image" src="image.png"><script src="local.js"></script></body></html>'),
    writeFile(path.join(root, 'local.js'), "document.querySelector('#proof').textContent='Windows sibling JavaScript loaded';document.querySelector('#run').onclick=()=>{document.querySelector('#proof').textContent='Remote script ran on Android';};"),
    writeFile(path.join(root, 'style.css'), 'body{background:#17171b;color:#ddd;font:16px system-ui}h1{color:#77c3ff}img{max-width:95%}button{padding:10px;background:#303039;color:#fff}'),
    writeFile(path.join(root, 'image.png'), qaImage()),
    writeFile(path.join(root, 'transfer.bin'), binary),
    writeFile(path.join(root, 'host-subfolder', 'nested.txt'), 'This nested file is on the Windows disk.\n'),
  ]);
}
async function digest(file: string): Promise<string> {
  const hash = createHash('sha256'); for await (const bytes of createReadStream(file)) hash.update(bytes); return hash.digest('hex');
}
export async function remoteHostSnapshot(root: string): Promise<Array<{ name: string; size: number; sha256: string; png?: { width: number; height: number } }>> {
  const entries = await readdir(root, { withFileTypes: true });
  const result = [];
  for (const entry of entries.slice(0, 64)) if (entry.isFile() && !entry.name.startsWith('.shellfox-')) {
    const file = path.join(root, entry.name), info = await stat(file);
    let png: { width: number; height: number } | undefined;
    if (/\.png$/i.test(entry.name)) {
      const handle = await open(file, 'r');
      try { const bytes = Buffer.alloc(24); const read = await handle.read(bytes, 0, bytes.length, 0); if (read.bytesRead === 24 && bytes.subarray(0, 8).toString('hex') === '89504e470d0a1a0a') png = { width: bytes.readUInt32BE(16), height: bytes.readUInt32BE(20) }; }
      finally { await handle.close(); }
    }
    result.push({ name: entry.name, size: info.size, sha256: await digest(file), ...(png ? { png } : {}) });
  }
  return result;
}
