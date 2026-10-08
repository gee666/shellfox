import path from 'node:path';
import { inflateRawSync } from 'node:zlib';

// Validate both central and local names before ditto can write anything. Electron
// frameworks use symlinks, so allow relative links confined to Shellfox.app, but
// never permit an archive entry beneath a symlink. ZIP64/encrypted archives are
// not supported by this updater.
export function validateUpdateZip(zip: Buffer): void {
  const fail = () => { throw new Error('Unsafe or unsupported update archive.'); };
  let end = -1;
  for (let i = zip.length - 22; i >= Math.max(0, zip.length - 65557); i--) {
    if (zip.readUInt32LE(i) === 0x06054b50 && i + 22 + zip.readUInt16LE(i + 20) === zip.length) { end = i; break; }
  }
  if (end < 0 || zip.readUInt16LE(end + 4) || zip.readUInt16LE(end + 6)) return fail();
  const entries = zip.readUInt16LE(end + 10), size = zip.readUInt32LE(end + 12), start = zip.readUInt32LE(end + 16);
  if (!entries || entries === 0xffff || zip.readUInt16LE(end + 8) !== entries || start + size !== end) return fail();
  const names = new Set<string>(), links = new Map<string, string>();
  let cursor = start, expanded = 0;
  for (let i = 0; i < entries; i++) {
    if (cursor + 46 > end || zip.readUInt32LE(cursor) !== 0x02014b50) return fail();
    const flags = zip.readUInt16LE(cursor + 8), method = zip.readUInt16LE(cursor + 10);
    const packed = zip.readUInt32LE(cursor + 20), unpacked = zip.readUInt32LE(cursor + 24);
    const length = zip.readUInt16LE(cursor + 28), extra = zip.readUInt16LE(cursor + 30), comment = zip.readUInt16LE(cursor + 32);
    const mode = zip.readUInt32LE(cursor + 38) >>> 16, local = zip.readUInt32LE(cursor + 42);
    const next = cursor + 46 + length + extra + comment;
    if (next > end || flags & 1 || ![0, 8].includes(method) || local + 30 > start || zip.readUInt32LE(local) !== 0x04034b50 || zip.readUInt16LE(local + 6) !== flags || zip.readUInt16LE(local + 8) !== method) return fail();
    const bytes = zip.subarray(cursor + 46, cursor + 46 + length), name = bytes.toString('utf8');
    const normalized = name.replace(/\/$/, '');
    if (!name || !Buffer.from(name).equals(bytes) || /[\\\u0000-\u001f\u007f]/.test(name) || !['Shellfox.app', '__MACOSX'].includes(name.split('/')[0]!) || normalized.split('/').some(p => !p || p === '.' || p === '..') || names.has(normalized)) return fail();
    const folded = normalized.toLowerCase();
    if (names.has(folded)) return fail();
    names.add(folded);
    const localLength = zip.readUInt16LE(local + 26), localExtra = zip.readUInt16LE(local + 28);
    const data = local + 30 + localLength + localExtra;
    if (localLength !== length || !bytes.equals(zip.subarray(local + 30, local + 30 + localLength)) || data + packed > start || packed === 0xffffffff || unpacked === 0xffffffff) return fail();
    expanded += unpacked;
    if (expanded > 3 * 1024 * 1024 * 1024) return fail();
    const type = mode & 0o170000;
    if (type && ![0o100000, 0o040000, 0o120000].includes(type)) return fail();
    if (type === 0o120000) {
      if (unpacked > 4096 || packed > 8192 || name.endsWith('/') || !name.startsWith('Shellfox.app/')) return fail();
      const dataBytes = zip.subarray(data, data + packed);
      const targetBytes = method === 8 ? inflateRawSync(dataBytes, { maxOutputLength: 4096 }) : dataBytes;
      const target = targetBytes.toString('utf8');
      const resolved = path.posix.join(path.posix.dirname(name), target);
      if (!target || targetBytes.length !== unpacked || !Buffer.from(target).equals(targetBytes) || /[\\\u0000-\u001f\u007f]/.test(target) || target.startsWith('/') || !resolved.startsWith('Shellfox.app/')) return fail();
      links.set(folded, target);
    }
    cursor = next;
  }
  if (cursor !== end || !names.has('shellfox.app/contents/info.plist') || !names.has('shellfox.app/contents/macos/shellfox')) return fail();
  // Resolve link chains without normalizing away '..' before following links.
  // Otherwise a link to another directory link can escape despite a lexical check.
  for (const [name, target] of links) {
    const pending = [...path.posix.dirname(name).split('/'), ...target.toLowerCase().split('/')];
    const resolved: string[] = [];
    let hops = 0;
    while (pending.length) {
      const part = pending.shift()!;
      if (!part || part === '.') continue;
      if (part === '..') { if (resolved.length <= 1) return fail(); resolved.pop(); continue; }
      resolved.push(part);
      if (resolved[0] !== 'shellfox.app') return fail();
      const next = links.get(resolved.join('/'));
      if (next) {
        if (++hops > 64) return fail();
        resolved.pop(); pending.unshift(...next.toLowerCase().split('/'));
      }
    }
  }
  for (const name of names) {
    let parent = path.posix.dirname(name);
    while (parent !== '.') { if (links.has(parent)) return fail(); parent = path.posix.dirname(parent); }
  }
}
