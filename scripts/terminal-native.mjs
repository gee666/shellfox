// Build-time only. The installed runtime never compiles native helpers.
import { chmodSync, mkdirSync, rmSync } from 'node:fs';
import path from 'node:path';
import { root, run, runNode } from './common.mjs';
import { darwinCompilerArgs, darwinHelpers } from './terminal-native-helpers.mjs';
if (process.platform === 'win32') {
  if (!['x64', 'arm64'].includes(process.arch)) throw new Error('Windows elevated helper requires x64 or arm64.');
  const output = path.join(root, 'tmp/terminal-native', 'win-' + process.arch);
  rmSync(output, { recursive: true, force: true });
  run('dotnet', ['publish', path.join(root, 'native/Shellfox.ElevatedTerminal/Shellfox.ElevatedTerminal.csproj'), '-c', 'Release', '-r', 'win-' + process.arch, '--self-contained', 'true', '-o', output], 300000);
  run(path.join(output, 'Shellfox.ElevatedTerminal.exe'), ['probe'], 15000);
} else if (process.platform !== 'darwin') {
  console.log('No Darwin native helper build on ' + process.platform + '.');
} else {
  if (!['x64', 'arm64'].includes(process.arch)) throw new Error('Darwin native helpers require x64 or arm64.');
  const output = path.join(root, 'tmp/terminal-native', 'darwin-' + process.arch);
  mkdirSync(output, { recursive: true });
  // Recreate outputs each build, never accept a stale or cross-architecture helper.
  for (const helper of darwinHelpers) {
    const file = path.join(output, helper.basename);
    rmSync(file, { force: true });
    run('xcrun', darwinCompilerArgs(root, process.arch, helper), 120000);
    chmodSync(file, 0o755);
    run('codesign', ['--force', '--sign', '-', '--timestamp=none', file], 60000);
  }
  runNode('scripts/terminal-native-preflight.mjs', [output], 60000);
}
