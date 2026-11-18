import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { aotToolchain, windowsBuildOptions } from './terminal-aot.mjs';
import { nativePeAudit } from './terminal-pe.mjs';
import { projectRoot } from './dotnet-sdk.mjs';

test('NativeAOT is opt-in and architecture flags cannot silently switch or fall back', () => {
  assert.deepEqual(windowsBuildOptions([], 'arm64'), { arch: 'arm64', aot: false });
  assert.deepEqual(windowsBuildOptions(['--aot', '--arch', 'x64'], 'arm64'), { arch: 'x64', aot: true });
  assert.deepEqual(windowsBuildOptions(['--arch', 'arm64', '--aot'], 'x64'), { arch: 'arm64', aot: true });
  for (const args of [['--aot', '--aot'], ['--arch'], ['--arch', 'x86'], ['--unknown'], ['--arch', 'x64', '--arch', 'arm64']])
    assert.throws(() => windowsBuildOptions(args));
});
test('toolchain checks target-specific MSVC/SDK inputs without executing fixture tools or changing PATH', () => {
  const temporary = path.join(projectRoot, 'tmp'); mkdirSync(temporary, { recursive: true });
  const root = mkdtempSync(path.join(temporary, 'aot-tools-test-'));
  try {
    const msvc = path.join(root, 'msvc'), kits = path.join(root, 'kits');
    for (const name of ['msvc/bin/Hostx64/x64/link.exe', 'msvc/lib/x64/libcmt.lib', 'kits/Lib/10.0.22000.0/um/x64/kernel32.lib', 'kits/Lib/10.0.22000.0/ucrt/x64/libucrt.lib']) {
      const file = path.join(root, name); mkdirSync(path.dirname(file), { recursive: true }); writeFileSync(file, 'inert owned fixture');
    }
    const before = process.env.PATH;
    const result = aotToolchain('x64', { msvc, kits, hostArch: 'x64' });
    assert.equal(result.evidence.target, 'x64'); assert.equal(result.evidence.windowsSdkVersion, '10.0.22000.0');
    assert.equal(process.env.PATH, before); assert.ok(result.environment.PATH.startsWith(path.join(msvc, 'bin/Hostx64/x64')));
    assert.throws(() => aotToolchain('arm64', { msvc, kits, hostArch: 'x64' }), /arm64 MSVC/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
function image(arch = 'x64') {
  const bytes = Buffer.alloc(0xa00), pe = 0x80, optional = pe + 24;
  bytes.writeUInt16LE(0x5a4d, 0); bytes.writeUInt32LE(pe, 0x3c); bytes.writeUInt32LE(0x4550, pe);
  bytes.writeUInt16LE(arch === 'x64' ? 0x8664 : 0xaa64, pe + 4); bytes.writeUInt16LE(1, pe + 6);
  bytes.writeUInt16LE(240, pe + 20); bytes.writeUInt16LE(2, pe + 22); bytes.writeUInt16LE(0x20b, optional);
  bytes.writeUInt16LE(0x140, optional + 70); bytes.writeUInt32LE(16, optional + 108);
  const directory = (index, rva, size) => { bytes.writeUInt32LE(rva, optional + 112 + index * 8); bytes.writeUInt32LE(size, optional + 116 + index * 8); };
  directory(1, 0x1000, 40); directory(10, 0x1200, 80);
  const section = optional + 240;
  bytes.writeUInt32LE(0x800, section + 8); bytes.writeUInt32LE(0x1000, section + 12);
  bytes.writeUInt32LE(0x800, section + 16); bytes.writeUInt32LE(0x200, section + 20);
  bytes.writeUInt32LE(0x1100, 0x200 + 12); bytes.write('KERNEL32.dll\0', 0x300, 'ascii');
  bytes.writeUInt32LE(80, 0x400); bytes.writeUInt16LE(0x800, 0x400 + 78);
  return { bytes, optional, directory };
}
test('PE proof requires native machine, no CLR header, NX/ASLR, System32 dependencies and load flags', () => {
  for (const arch of ['x64', 'arm64']) {
    const { bytes } = image(arch); const result = nativePeAudit(bytes, arch);
    assert.equal(result.clrDirectory, 'absent'); assert.equal(result.dependentLoadFlags, '0x800');
    assert.deepEqual(result.directImports, ['kernel32.dll']);
  }
  assert.throws(() => nativePeAudit(image().bytes, 'arm64'), /machine/);
  const managed = image(); managed.directory(14, 0x1300, 72);
  assert.throws(() => nativePeAudit(managed.bytes, 'x64'), /CLR/);
  const unsafe = image(); unsafe.bytes.writeUInt16LE(0, 0x400 + 78);
  assert.throws(() => nativePeAudit(unsafe.bytes, 'x64'), /DEPENDENTLOADFLAG/);
  const redirect = image(); redirect.bytes.fill(0, 0x300, 0x340); redirect.bytes.write('vcruntime140.dll\0', 0x300);
  assert.throws(() => nativePeAudit(redirect.bytes, 'x64'), /Unreviewed/);
  const nonAscii = image(); nonAscii.bytes[0x300] = 0xcb;
  assert.throws(() => nativePeAudit(nonAscii.bytes, 'x64'), /ASCII/);
  assert.throws(() => nativePeAudit(Buffer.alloc(16), 'x64'));
});
test('delay imports and malformed RVA/directory bounds cannot bypass the same policy', () => {
  const delayed = image(); delayed.directory(13, 0x1300, 64);
  delayed.bytes.writeUInt32LE(1, 0x500); delayed.bytes.writeUInt32LE(0x1400, 0x504); delayed.bytes.write('USER32.dll\0', 0x600);
  assert.deepEqual(nativePeAudit(delayed.bytes, 'x64').delayedImports, ['user32.dll']);
  delayed.bytes.writeUInt32LE(0, 0x500); assert.throws(() => nativePeAudit(delayed.bytes, 'x64'), /RVA/);
  const outside = image(); outside.directory(1, 0xfffffff0, 40);
  assert.throws(() => nativePeAudit(outside.bytes, 'x64'), /RVA/);
});
