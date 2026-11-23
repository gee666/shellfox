import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { assertGlibcFloor, compareGlibc, GLIBC_FLOOR, requiredGlibc } from './package-rust-glibc.mjs';

/** Minimal ELF64 with a .gnu.version_r naming the given versions of libc.so.6. */
function elf(versions) {
  const strings = Buffer.from(['', 'libc.so.6', ...versions].join('\0') + '\0', 'latin1');
  const offset = value => strings.indexOf(Buffer.from('\0' + value + '\0', 'latin1')) + 1;
  const need = Buffer.alloc(16 + 16 * versions.length);
  need.writeUInt16LE(1, 0); need.writeUInt16LE(versions.length, 2); need.writeUInt32LE(offset('libc.so.6'), 4); need.writeUInt32LE(16, 8);
  versions.forEach((version, i) => {
    const at = 16 + 16 * i;
    need.writeUInt32LE(offset(version), at + 8);
    need.writeUInt32LE(i + 1 < versions.length ? 16 : 0, at + 12);
  });
  const header = Buffer.alloc(64), sectionsAt = 64 + strings.length + need.length;
  header.writeUInt32BE(0x7f454c46, 0); header[4] = 2; header[5] = 1;
  header.writeBigUInt64LE(BigInt(sectionsAt), 0x28); header.writeUInt16LE(64, 0x3a); header.writeUInt16LE(3, 0x3c);
  const sections = Buffer.alloc(64 * 3);
  sections.writeUInt32LE(3, 64 + 4); sections.writeBigUInt64LE(64n, 64 + 0x18); // SHT_STRTAB
  sections.writeUInt32LE(0x6ffffffe, 128 + 4); sections.writeBigUInt64LE(BigInt(64 + strings.length), 128 + 0x18);
  sections.writeUInt32LE(1, 128 + 0x28); sections.writeUInt32LE(1, 128 + 0x2c);
  return Buffer.concat([header, strings, need, sections]);
}

test('reads the highest glibc symbol version an ELF binary needs', () => {
  assert.equal(requiredGlibc(elf(['GLIBC_2.2.5', 'GLIBC_2.17', 'GLIBC_2.31', 'GLIBC_2.4'])), '2.31');
  assert.equal(requiredGlibc(elf(['GLIBC_PRIVATE'])), null);
  assert.ok(compareGlibc('2.2.5', '2.17') < 0 && compareGlibc('2.39', '2.31') > 0);
  assert.throws(() => requiredGlibc(Buffer.from('#!/bin/sh\n')), /ELF/);
});

test('packaging fails on binaries newer than the glibc floor', () => {
  const directory = mkdtempSync(path.join(import.meta.dirname, '../tmp/glibc-'));
  try {
    const old = path.join(directory, 'old'), current = path.join(directory, 'new');
    writeFileSync(old, elf(['GLIBC_2.17', `GLIBC_${GLIBC_FLOOR}`]));
    writeFileSync(current, elf(['GLIBC_2.17', 'GLIBC_2.39']));
    assert.doesNotThrow(() => assertGlibcFloor([old]));
    assert.throws(() => assertGlibcFloor([old, current]), /needs GLIBC_2\.39/);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});
