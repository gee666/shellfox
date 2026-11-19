import assert from 'node:assert/strict';
import test from 'node:test';
import { verifyWslFilesElf, wslFilesTarget } from './rust-package-wsl-files.mjs';

test('packaged WSL worker must be static, executable and match the native package', () => {
  const elf = Buffer.alloc(120);
  elf.set([0x7f, 0x45, 0x4c, 0x46, 2, 1]);
  elf.writeUInt16LE(62, 18);
  elf.writeBigUInt64LE(64n, 32);
  elf.writeUInt16LE(56, 54);
  elf.writeUInt16LE(1, 56);
  elf.writeUInt32LE(1, 64); // PT_LOAD
  elf.writeUInt32LE(5, 68); // PF_X | PF_R
  assert.doesNotThrow(() => verifyWslFilesElf(elf, wslFilesTarget('x64')));
  assert.throws(() => verifyWslFilesElf(elf, wslFilesTarget('arm64')), /matching ELF/);
  elf.writeUInt16LE(183, 18);
  assert.doesNotThrow(() => verifyWslFilesElf(elf, wslFilesTarget('arm64')));
  elf.writeUInt32LE(3, 64); // PT_INTERP
  assert.throws(() => verifyWslFilesElf(elf, wslFilesTarget('arm64')), /dynamic loader/);
  assert.throws(() => verifyWslFilesElf(elf.subarray(0, 65), wslFilesTarget('arm64')), /program headers/);
});
