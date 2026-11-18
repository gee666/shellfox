// Bounded PE audit for a trusted NativeAOT build, not a way to trust an arbitrary EXE.
import { lstatSync, readFileSync } from 'node:fs';

const osLibraries = new Set(['advapi32.dll', 'bcrypt.dll', 'kernel32.dll', 'ole32.dll', 'user32.dll']);
const crtApiSet = /^api-ms-win-crt-(?:math|heap|string|convert|stdio|runtime|locale)-l1-1-0\.dll$/;
export function nativePeAudit(bytes, arch) {
  function need(offset, size) {
    if (!Number.isSafeInteger(offset) || !Number.isSafeInteger(size) || offset < 0 || size < 0 || offset + size > bytes.length)
      throw new Error('PE field exceeds file bounds.');
  }
  const u16 = offset => { need(offset, 2); return bytes.readUInt16LE(offset); };
  const u32 = offset => { need(offset, 4); return bytes.readUInt32LE(offset); };
  if (bytes.length > 256 * 1024 * 1024 || u16(0) !== 0x5a4d) throw new Error('Invalid NativeAOT DOS image.');
  const pe = u32(0x3c); need(pe, 24);
  if (u32(pe) !== 0x4550) throw new Error('Invalid PE signature.');
  const machine = u16(pe + 4), sections = u16(pe + 6), optionalSize = u16(pe + 20), optional = pe + 24;
  if ((u16(pe + 22) & 0x2002) !== 2) throw new Error('Expected an executable image, not a DLL.');
  if (machine !== ({ x64: 0x8664, arm64: 0xaa64 })[arch] || sections < 1 || sections > 96 || optionalSize < 240)
    throw new Error('NativeAOT machine/header mismatch.');
  need(optional, optionalSize);
  if (u16(optional) !== 0x20b || u32(optional + 108) < 16) throw new Error('Expected PE32+ directory table.');
  const characteristics = u16(optional + 70);
  if ((characteristics & 0x140) !== 0x140) throw new Error('Native image must retain ASLR and NX.');
  const directory = index => ({ rva: u32(optional + 112 + index * 8), size: u32(optional + 116 + index * 8) });
  const clr = directory(14);
  if (clr.rva !== 0 || clr.size !== 0) throw new Error('Managed CLR directory is not a NativeAOT artifact.');
  const table = optional + optionalSize; need(table, sections * 40);
  function span(rva, size) {
    for (let i = 0; i < sections; i++) {
      const section = table + i * 40;
      const address = u32(section + 12), rawSize = u32(section + 16), raw = u32(section + 20);
      if (rva >= address && rva - address < rawSize && rva - address + size <= rawSize) {
        const offset = raw + rva - address; need(offset, size); return { offset, end: raw + rawSize };
      }
    }
    throw new Error('PE RVA is not backed by a bounded section.');
  }
  function library(rva) {
    const mapped = span(rva, 1), limit = Math.min(bytes.length, mapped.end, mapped.offset + 261);
    let end = mapped.offset;
    while (end < limit && bytes[end] !== 0) end++;
    if (end === limit || end === mapped.offset) throw new Error('Unbounded PE import name.');
    if (bytes.subarray(mapped.offset, end).some(value => value > 127)) throw new Error('Non-ASCII PE import name.');
    const name = bytes.toString('ascii', mapped.offset, end).toLowerCase();
    if (!/^[a-z0-9_.-]+\.dll$/.test(name) || !osLibraries.has(name) && !crtApiSet.test(name))
      throw new Error(`Unreviewed native dependency: ${name}`);
    return name;
  }
  function imports(index, width, nameOffset, delay = false) {
    const value = directory(index);
    if (value.rva === 0 && value.size === 0) return [];
    if (value.size < width || value.size > 1024 * 1024) throw new Error('Invalid PE import directory bound.');
    const result = [];
    for (let offset = 0; offset + width <= value.size && result.length < 512; offset += width) {
      const start = span(value.rva + offset, width).offset;
      if (bytes.subarray(start, start + width).every(value => value === 0)) return [...new Set(result)].sort();
      if (delay && u32(start) !== 1) throw new Error('Only RVA-based delayed imports are supported.');
      result.push(library(u32(start + nameOffset)));
    }
    throw new Error('PE import list has no bounded terminator.');
  }
  const direct = imports(1, 20, 12), delayed = imports(13, 32, 4, true);
  if (!direct.length) throw new Error('Expected native OS imports.');
  const config = directory(10);
  if (config.size < 80) throw new Error('Missing PE dependent-load policy.');
  const configOffset = span(config.rva, config.size).offset;
  if (u32(configOffset) < 80 || u32(configOffset) > config.size || u16(configOffset + 78) !== 0x800)
    throw new Error('NativeAOT direct imports must use DEPENDENTLOADFLAG:0x800.');
  return {
    format: 'shellfox-native-aot-pe-audit-v1', arch, machine, clrDirectory: 'absent',
    directImports: direct, delayedImports: delayed, dependentLoadFlags: '0x800', aslr: true, nx: true,
    minimumDependentLoadPolicyWindowsBuild: 14393, helperMinimumWindowsBuild: 17763,
    coverage: 'PE/import policy only. NativeAOT identity also requires trusted compiler/source evidence, fixed lazy-import review and exact artifact hashes.',
  };
}
export function auditNativePe(file, arch) {
  const stat = lstatSync(file);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.nlink !== 1 || stat.size > 256 * 1024 * 1024)
    throw new Error('Expected bounded regular single-link NativeAOT image.');
  return nativePeAudit(readFileSync(file), arch);
}
