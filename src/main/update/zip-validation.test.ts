import { describe, expect, it } from 'vitest';
import { validateUpdateZip } from './zip-validation';
interface Entry { name: string; data?: string; mode?: number; localName?: string }
function archive(extra: Entry[] = []): Buffer {
  const entries: Entry[] = [{ name: 'Shellfox.app/Contents/Info.plist' }, { name: 'Shellfox.app/Contents/MacOS/Shellfox' }, ...extra];
  const locals: Buffer[] = [], centrals: Buffer[] = [];
  let offset = 0;
  for (const e of entries) {
    const name = Buffer.from(e.name), localName = Buffer.from(e.localName ?? e.name), data = Buffer.from(e.data ?? 'file');
    const local = Buffer.alloc(30); local.writeUInt32LE(0x04034b50); local.writeUInt32LE(data.length, 18); local.writeUInt32LE(data.length, 22); local.writeUInt16LE(localName.length, 26);
    const central = Buffer.alloc(46); central.writeUInt32LE(0x02014b50); central.writeUInt32LE(data.length, 20); central.writeUInt32LE(data.length, 24); central.writeUInt16LE(name.length, 28); central.writeUInt32LE(((e.mode ?? 0o100644) << 16) >>> 0, 38); central.writeUInt32LE(offset, 42);
    locals.push(local, localName, data); centrals.push(central, name); offset += local.length + localName.length + data.length;
  }
  const end = Buffer.alloc(22), central = Buffer.concat(centrals); end.writeUInt32LE(0x06054b50); end.writeUInt16LE(entries.length, 8); end.writeUInt16LE(entries.length, 10); end.writeUInt32LE(central.length, 12); end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, central, end]);
}
describe('macOS update archive validation', () => {
  it('accepts app entries and confined framework links', () => {
    expect(() => validateUpdateZip(archive([{ name: 'Shellfox.app/Contents/Frameworks/Test.framework/Versions/Current', mode: 0o120777, data: 'A' }, { name: 'Shellfox.app/Contents/Frameworks/Test.framework/Test', mode: 0o120777, data: 'Versions/Current/Test' }, { name: 'Shellfox.app/Contents/Frameworks/Test.framework/Versions/A/Test' }]))).not.toThrow();
  });
  it.each(['/etc/passwd', '../Shellfox.app/escape', 'Shellfox.app/../escape', 'Shellfox.app//escape', 'Shellfox.app/./escape', 'Shellfox.app\\escape', 'Shellfox.app/escape\0', 'Other.app/file'])('rejects unsafe entry %j', name => {
    expect(() => validateUpdateZip(archive([{ name }]))).toThrow();
  });
  it.each(['/etc/passwd', '../../../outside', '..\\outside', 'x\0'])('rejects unsafe link %j', data => {
    expect(() => validateUpdateZip(archive([{ name: 'Shellfox.app/Contents/link', mode: 0o120777, data }]))).toThrow();
  });
  it('rejects writes beneath links, including case-insensitive aliases', () => {
    expect(() => validateUpdateZip(archive([{ name: 'Shellfox.app/Contents/link', mode: 0o120777, data: 'other' }, { name: 'Shellfox.app/Contents/LINK/file' }]))).toThrow();
  });
  it('rejects link chains that escape after following a directory link, and cycles', () => {
    expect(() => validateUpdateZip(archive([{ name: 'Shellfox.app/d/e/link', mode: 0o120777, data: 'other/../../outside' }, { name: 'Shellfox.app/d/e/other', mode: 0o120777, data: '../../shallow' }]))).toThrow();
    expect(() => validateUpdateZip(archive([{ name: 'Shellfox.app/a', mode: 0o120777, data: 'b' }, { name: 'Shellfox.app/b', mode: 0o120777, data: 'a' }]))).toThrow();
  });
  it('rejects duplicate entries and mismatched local names', () => {
    expect(() => validateUpdateZip(archive([{ name: 'Shellfox.app/Contents/Info.plist' }]))).toThrow();
    expect(() => validateUpdateZip(archive([{ name: 'Shellfox.app/contents/INFO.plist' }]))).toThrow();
    expect(() => validateUpdateZip(archive([{ name: 'Shellfox.app/safe', localName: '../unsafe' }]))).toThrow();
  });
  it('rejects truncated, malformed and ZIP64 archives', () => {
    const good = archive(); expect(() => validateUpdateZip(good.subarray(0, good.length - 2))).toThrow();
    const bad = Buffer.from(good); bad.writeUInt16LE(0xffff, bad.length - 12); expect(() => validateUpdateZip(bad)).toThrow();
    expect(() => validateUpdateZip(Buffer.alloc(2))).toThrow();
  });
});
