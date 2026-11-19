import { readFileSync } from 'node:fs';
import path from 'node:path';
import { run } from './rust-package-common.mjs';

export function wslFilesTarget(arch = process.arch) {
  if (arch === 'x64') return 'x86_64-unknown-linux-musl';
  if (arch === 'arm64') return 'aarch64-unknown-linux-musl';
  throw new Error(`WSL Files packaging does not support ${arch}`);
}

export function verifyWslFilesElf(bytes, target) {
  const machine = target === 'x86_64-unknown-linux-musl' ? 62 : 183;
  if (bytes.length < 64 || bytes.length > 32 * 1024 * 1024 ||
      !bytes.subarray(0, 6).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46, 2, 1])) ||
      bytes.readUInt16LE(18) !== machine) throw new Error(`Expected a matching ELF for ${target}`);
  const offset = bytes.readBigUInt64LE(32), size = bytes.readUInt16LE(54), count = bytes.readUInt16LE(56);
  if (!count || size !== 56 || offset < 64n || offset + BigInt(size * count) > BigInt(bytes.length)) {
    throw new Error('Invalid WSL Files ELF program headers');
  }
  let executable = false;
  for (let i = 0; i < count; i++) {
    const header = Number(offset) + i * size;
    const type = bytes.readUInt32LE(header);
    if (type === 3) throw new Error('Packaged WSL Files must not require a guest dynamic loader');
    if (type === 1 && (bytes.readUInt32LE(header + 4) & 1)) executable = true;
  }
  if (!executable) throw new Error('Expected executable code in WSL Files ELF');
}

export function buildWslFilesWorker(work) {
  const target = wslFilesTarget();
  const key = `CARGO_TARGET_${target.replaceAll('-', '_').toUpperCase()}`;
  const output = path.join(work, 'wsl-files-build');
  run('cargo', ['build', '--locked', '--release', '--target', target, '--target-dir', output,
    '-p', 'shellfox-files', '--bin', 'shellfox-wsl-files'], {
    env: { [`${key}_LINKER`]: 'rust-lld', [`${key}_RUSTFLAGS`]: '-C linker-flavor=ld.lld -C link-self-contained=yes -C strip=symbols' },
  });
  const binary = path.join(output, target, 'release', 'shellfox-wsl-files');
  verifyWslFilesElf(readFileSync(binary), target);
  return binary;
}
