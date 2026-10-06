import { existsSync } from 'node:fs';
import path from 'node:path';
import { run, root } from './common.mjs';
const mode = process.argv[2];
const linux = process.argv.includes('--linux') || process.platform === 'linux';
if (mode === 'publish') {
  const project = linux ? 'native/Shellfox.Linux/Shellfox.Linux.csproj' : 'native/Shellfox.Native/Shellfox.Native.csproj';
  const rid = linux ? 'linux-x64' : 'win-x64';
  run('dotnet', ['publish', project, '-c', 'Release', '-r', rid, '--self-contained', 'true', '-o', path.join(root, 'tmp/native', rid)]);
} else if (mode === 'test') {
  if (linux) { run('dotnet', ['run', '--project', 'native/Shellfox.Linux/Shellfox.Linux.csproj', '-c', 'Release', '--', '--self-test']); process.exit(0); }
  const project = 'native/Shellfox.Native.Tests/Shellfox.Native.Tests.csproj';
  if (!existsSync(project)) throw new Error('Native test project has not been published by its owner: ' + project);
  run('dotnet', ['test', project, '-c', 'Release', '--logger', 'trx;LogFileName=native.trx', '--results-directory', path.join(root, 'tmp/reports/native')]);
} else throw new Error('Unknown native command');
