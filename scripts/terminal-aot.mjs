// Windows NativeAOT build prerequisites. All environment changes are child-only.
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { projectRoot, runtimeVersion, sdkVersion } from './dotnet-sdk.mjs';
import { acquireNugetSnapshot, auditManagedInventory } from './rust-package-resource-inventory.mjs';

export function windowsBuildOptions(args, nativeArch = process.arch) {
  let arch = nativeArch, aot = false;
  const seen = new Set();
  for (let i = 0; i < args.length; i++) {
    const flag = args[i];
    if (seen.has(flag)) throw new Error('Duplicate Windows helper build flag.');
    seen.add(flag);
    if (flag === '--aot') aot = true;
    else if (flag === '--arch') arch = args[++i];
    else throw new Error('Expected [--arch x64|arm64] [--aot].');
  }
  if (!['x64', 'arm64'].includes(arch)) throw new Error('Expected --arch x64 or --arch arm64.');
  return { arch, aot };
}
function latest(names) {
  return names.filter(name => /^\d+(?:\.\d+)+$/.test(name)).sort((left, right) => {
    const a = left.split('.').map(Number), b = right.split('.').map(Number);
    for (let i = 0; i < Math.max(a.length, b.length); i++) if ((a[i] ?? 0) !== (b[i] ?? 0)) return (b[i] ?? 0) - (a[i] ?? 0);
    return 0;
  })[0];
}
function installedMsvc() {
  const programFiles = process.env['ProgramFiles(x86)'] || 'C:\\Program Files (x86)';
  const vswhere = path.join(programFiles, 'Microsoft Visual Studio/Installer/vswhere.exe');
  const result = spawnSync(vswhere, ['-latest', '-products', '*', '-version', '[17.0,)', '-property', 'installationPath'],
    { encoding: 'utf8', timeout: 15000, windowsHide: true });
  if (result.error || result.status !== 0 || !result.stdout.trim()) throw new Error('NativeAOT requires installed VS2022+ C++ tools; no installer is run.');
  const vc = path.join(result.stdout.trim(), 'VC');
  const version = readFileSync(path.join(vc, 'Auxiliary/Build/Microsoft.VCToolsVersion.default.txt'), 'utf8').trim();
  return path.join(vc, 'Tools/MSVC', version);
}
export function aotToolchain(arch, options = {}) {
  const host = options.hostArch ?? process.arch;
  if (!['x64', 'arm64'].includes(host) || !['x64', 'arm64'].includes(arch)) throw new Error('NativeAOT requires Windows x64/Arm64 tools.');
  const msvc = options.msvc ?? process.env.SHELLFOX_MSVC_ROOT ?? installedMsvc();
  const kits = options.kits ?? process.env.SHELLFOX_WINDOWS_KITS_ROOT ?? path.join(process.env['ProgramFiles(x86)'] || 'C:\\Program Files (x86)', 'Windows Kits/10');
  if (!path.isAbsolute(msvc) || !path.isAbsolute(kits)) throw new Error('Explicit NativeAOT C++ tool roots must be absolute.');
  const bin = path.join(msvc, `bin/Host${host}/${arch}`);
  const crt = path.join(msvc, `lib/${arch}`);
  if (!existsSync(path.join(bin, 'link.exe')) || !existsSync(path.join(crt, 'libcmt.lib')))
    throw new Error(`NativeAOT ${arch} MSVC tools/static CRT are missing. Install/provide the VS2022+ ${arch} C++ component separately; managed publishing is unchanged.`);
  const version = latest(readdirSync(path.join(kits, 'Lib')).filter(version =>
    existsSync(path.join(kits, `Lib/${version}/um/${arch}/kernel32.lib`)) && existsSync(path.join(kits, `Lib/${version}/ucrt/${arch}/libucrt.lib`))));
  if (!version) throw new Error(`NativeAOT ${arch} Windows SDK libraries are missing.`);
  const search = [bin, path.join(kits, `bin/${version}/${host}`), process.env.PATH ?? process.env.Path ?? ''].join(path.delimiter);
  return {
    environment: { PATH: search, Path: search, LIB: [crt, path.join(kits, `Lib/${version}/um/${arch}`), path.join(kits, `Lib/${version}/ucrt/${arch}`)].join(path.delimiter) },
    evidence: { host, target: arch, msvc, windowsSdk: kits, windowsSdkVersion: version, linker: path.join(bin, 'link.exe') },
  };
}
export async function writeAotEvidence(directory, intermediate, toolchain, peAudit) {
  const assetsFile = path.join(intermediate, 'project.assets.json');
  const assetsBytes = readFileSync(assetsFile), assets = JSON.parse(assetsBytes);
  const packages = Object.entries(assets.libraries).map(([id, value]) => {
    const split = id.lastIndexOf('/');
    return { name: id.slice(0, split), version: id.slice(split + 1), kind: value.type, nugetSha512: value.sha512 ?? null };
  });
  for (const value of Object.values(assets.project.frameworks).flatMap(value => value.downloadDependencies ?? [])) {
    const match = /^\[(\d+\.\d+\.\d+), \1\]$/.exec(value.version);
    if (!match) throw new Error('Exact NativeAOT build dependency versions required.');
    if (!packages.some(pkg => pkg.name === value.name && pkg.version === match[1]))
      packages.push({ name: value.name, version: match[1], kind: 'package', nugetSha512: null });
  }
  const runtimeName = `Microsoft.NETCore.App.Runtime.NativeAOT.win-${toolchain.target}`;
  for (const name of [runtimeName, 'Microsoft.DotNet.ILCompiler', `runtime.win-${toolchain.host}.Microsoft.DotNet.ILCompiler`])
    if (!packages.some(pkg => pkg.name === name && pkg.version === runtimeVersion)) throw new Error('NativeAOT compiler/runtime pin mismatch.');
  const runtimePack = path.join(projectRoot, 'tmp/build-cache/nuget', runtimeName.toLowerCase(), runtimeVersion);
  const notices = [];
  for (const name of ['LICENSE.TXT', 'THIRD-PARTY-NOTICES.TXT']) {
    const source = path.join(runtimePack, name), bytes = readFileSync(source);
    copyFileSync(source, path.join(directory, `NATIVEAOT-${name}`));
    notices.push({ sourcePackage: `${runtimeName}/${runtimeVersion}`, name, sha256: createHash('sha256').update(bytes).digest('hex') });
  }
  const snapshot = process.env.SHELLFOX_NUGET_VULNERABILITY_SNAPSHOT
    ? JSON.parse(readFileSync(process.env.SHELLFOX_NUGET_VULNERABILITY_SNAPSHOT, 'utf8'))
    : await acquireNugetSnapshot('10.0');
  const audit = auditManagedInventory({ runtimeVersion, packages }, snapshot);
  if (audit.status !== 'no-listed-findings' || audit.runtimeLifecycle.status !== 'current-in-listed-channel')
    throw new Error('NativeAOT NuGet/lifecycle audit did not clear.');
  for (const name of readdirSync(directory))
    if (/\.(dll|deps\.json|runtimeconfig\.json)$/i.test(name)) throw new Error('NativeAOT output contains a managed/runtime payload.');
  const sourceFiles = [...readdirSync(path.join(projectRoot, 'native/Shellfox.ElevatedTerminal'))
    .filter(name => /\.(cs|csproj)$/.test(name)).map(name => `native/Shellfox.ElevatedTerminal/${name}`),
    'native/Directory.Build.props', 'scripts/dotnet-sdk.mjs', 'scripts/terminal-native.mjs', 'scripts/terminal-aot.mjs', 'scripts/terminal-pe.mjs'].sort();
  const sourceInputs = sourceFiles.map(name => ({ path: name, sha256: createHash('sha256').update(readFileSync(path.join(projectRoot, name))).digest('hex') }));
  const linkFile = path.join(intermediate, 'Release/net10.0-windows', `win-${toolchain.target}`, 'native/link.rsp');
  const imageBytes = readFileSync(path.join(directory, 'Shellfox.ElevatedTerminal.exe'));
  const record = {
    format: 'shellfox-native-aot-build-evidence-v1', sdkVersion, embeddedRuntimeVersion: runtimeVersion,
    publishProfile: 'ShellfoxNativeAot', targetFramework: 'net10.0-windows',
    sourceInputs, sourceDigest: createHash('sha256').update(JSON.stringify(sourceInputs)).digest('hex'),
    image: { path: 'Shellfox.ElevatedTerminal.exe', bytes: imageBytes.length, sha256: createHash('sha256').update(imageBytes).digest('hex') },
    linkerResponse: { path: linkFile, sha256: createHash('sha256').update(readFileSync(linkFile)).digest('hex') },
    nativeLicenseReview: 'Pending review of MSVC/Windows SDK static inputs and toolchain support; .NET runtime and NativeAOT notices are retained below.',
    toolchain, compilerWarningsAreErrors: true, invariantGlobalization: true, reflectionJson: false,
    assetsMetadataSha256: createHash('sha256').update(assetsBytes).digest('hex'), packages, notices,
    peAudit, authorization: 'Prototype build evidence only. Not a runtime trust token or ARM64/UAC/power acceptance.',
  };
  writeFileSync(path.join(directory, 'native-aot-build.json'), JSON.stringify(record, null, 2) + '\n');
  writeFileSync(path.join(directory, 'native-aot-nuget-audit.json'), JSON.stringify(audit, null, 2) + '\n');
  return record;
}
