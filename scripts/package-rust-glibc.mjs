// Linux packages must run wherever Electron 0.9.15's deb did, or its updater
// installs a package that apt rejects on every start. Release builds run in an
// Ubuntu 20.04 container; this check fails if a binary still needs newer glibc.
import { readFileSync } from 'node:fs';

export const GLIBC_FLOOR = '2.31';

const parts = version => version.split('.').map(Number);
export function compareGlibc(a, b) {
  const [x, y] = [parts(a), parts(b)];
  for (let i = 0; i < Math.max(x.length, y.length); i++) if ((x[i] ?? 0) !== (y[i] ?? 0)) return (x[i] ?? 0) - (y[i] ?? 0);
  return 0;
}

/** Highest GLIBC_x.y an ELF64 little-endian file requires, or null (static or no glibc). */
export function requiredGlibc(buffer) {
  if (buffer.readUInt32BE(0) !== 0x7f454c46 || buffer[4] !== 2 || buffer[5] !== 1) throw new Error('Expected a 64-bit little-endian ELF file');
  const sectionsAt = Number(buffer.readBigUInt64LE(0x28)), size = buffer.readUInt16LE(0x3a), count = buffer.readUInt16LE(0x3c);
  const sections = Array.from({ length: count }, (_, i) => {
    const at = sectionsAt + i * size;
    return { type: buffer.readUInt32LE(at + 4), offset: Number(buffer.readBigUInt64LE(at + 0x18)), link: buffer.readUInt32LE(at + 0x28), info: buffer.readUInt32LE(at + 0x2c) };
  });
  const needed = sections.find(section => section.type === 0x6ffffffe); // SHT_GNU_verneed
  if (!needed) return null;
  const strings = sections[needed.link].offset;
  const name = at => buffer.toString('latin1', strings + at, buffer.indexOf(0, strings + at));
  let highest = null, entry = needed.offset;
  for (let n = 0; n < needed.info; n++) {
    let aux = entry + buffer.readUInt32LE(entry + 8);
    for (let i = buffer.readUInt16LE(entry + 2); i > 0; i--) {
      const version = /^GLIBC_(\d+(?:\.\d+)+)$/.exec(name(buffer.readUInt32LE(aux + 8)))?.[1];
      if (version && (!highest || compareGlibc(version, highest) > 0)) highest = version;
      const next = buffer.readUInt32LE(aux + 12);
      if (!next) break;
      aux += next;
    }
    const next = buffer.readUInt32LE(entry + 12);
    if (!next) break;
    entry += next;
  }
  return highest;
}

export function assertGlibcFloor(files, floor = GLIBC_FLOOR) {
  for (const file of files) {
    const version = requiredGlibc(readFileSync(file));
    if (version && compareGlibc(version, floor) > 0) throw new Error(`${file} needs GLIBC_${version}; Linux packages must run on glibc ${floor}`);
  }
}
