// Build-time only. The installed runtime never compiles native helpers.
import { chmodSync, mkdirSync, rmSync } from 'node:fs';
import path from 'node:path';
import { root, run, runNode } from './common.mjs';
import { darwinCompilerArgs, darwinHelpers } from './terminal-native-helpers.mjs';
import { assertSdk, dotnetEnvironment, helperEnvironment, retainRuntimeNotices } from './dotnet-sdk.mjs';
import { aotToolchain, windowsBuildOptions, writeAotEvidence } from './terminal-aot.mjs';
import { auditNativePe } from './terminal-pe.mjs';
if (process.platform !== 'win32' && process.argv.includes('--aot')) throw new Error('The optional NativeAOT profile is Windows-only.');
if (process.platform === 'win32') {
  if (!['x64', 'arm64'].includes(process.arch)) throw new Error('Windows elevated helper requires x64 or arm64.');
  const { arch, aot } = windowsBuildOptions(process.argv.slice(2));
  const tools = aot ? aotToolchain(arch) : null; // Fail before replacing any artifact.
  const output = path.join(root, aot ? 'tmp/terminal-native-aot' : 'tmp/terminal-native', 'win-' + arch);
  const dotnet = assertSdk();
  const args = ['publish', path.join(root, 'native/Shellfox.ElevatedTerminal/Shellfox.ElevatedTerminal.csproj'), '-c', 'Release', '-r', 'win-' + arch, '--self-contained', 'true', '-o', output];
  const cache = path.join(root, 'tmp/build-cache/Shellfox.ElevatedTerminal-aot', 'win-' + arch);
  if (aot) args.push('-p:ShellfoxNativeAot=true', '-p:IlcUseEnvironmentalTools=true',
    `-p:BaseIntermediateOutputPath=${path.join(cache, 'obj')}${path.sep}`, `-p:BaseOutputPath=${path.join(cache, 'bin')}${path.sep}`);
  rmSync(output, { recursive: true, force: true });
  run(dotnet, args, aot ? 540000 : 300000, { ...dotnetEnvironment(dotnet), ...tools?.environment });
  await retainRuntimeNotices(output);
  if (aot) {
    const pe = auditNativePe(path.join(output, 'Shellfox.ElevatedTerminal.exe'), arch);
    await writeAotEvidence(output, path.join(cache, 'obj'), tools.evidence, pe);
  }
  if (arch === process.arch) {
    const helper = path.join(output, 'Shellfox.ElevatedTerminal.exe');
    run(helper, ['probe'], 15000, helperEnvironment());
    run(helper, ['probe-isolated'], 15000, helperEnvironment());
    if (aot) run(helper, ['probe-power'], 15000, helperEnvironment());
  } else console.log('Cross-compiled win-' + arch + '; probes require a native runner.');
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
