// Active Windows helper only. Never installs an SDK globally or changes PATH.
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createReadStream, createWriteStream, existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { fileURLToPath } from 'node:url';

export const sdkVersion = '10.0.404';
export const runtimeVersion = '10.0.15';
export const channel = '10.0';
const runtimeNotices = [
  ['THIRD-PARTY-NOTICES.TXT', '66f1d4e44973185519bb4aa8a9718eb22fc7af2cc532e3ae9cfc4c127ee7fc54'],
  ['LICENSE.TXT', 'cfc21f5e8bd655ae997eec916138b707b1d290b83272c02a95c9f821b8c87310'],
];
export const projectRoot = fileURLToPath(new URL('../', import.meta.url));
const scratch = path.join(projectRoot, 'tmp', 'dotnet10');
const metadataUrl = `https://builds.dotnet.microsoft.com/dotnet/release-metadata/${channel}/releases.json`;
export const sdkDirectory = (arch = process.arch) => path.join(scratch, `win-${arch}`, sdkVersion);

export function dotnetCommand(override = process.env.SHELLFOX_DOTNET, arch = process.arch) {
  const command = override || path.join(sdkDirectory(arch), 'dotnet.exe');
  if (!path.isAbsolute(command) || path.basename(command).toLowerCase() !== 'dotnet.exe')
    throw new Error('SHELLFOX_DOTNET must be an absolute dotnet.exe path for the active helper.');
  if (!existsSync(command)) throw new Error('Provision the active helper SDK with node scripts/dotnet-sdk.mjs provision.');
  return command;
}
export function helperEnvironment(source = process.env) {
  return Object.fromEntries(Object.keys(source).filter(key => /^(?:DOTNET_|COMPLUS_|CORECLR_|COR_|DEVPATH$)/i.test(key)).map(key => [key, undefined]));
}
export function dotnetEnvironment(command = dotnetCommand()) {
  const removed = { ...helperEnvironment(), NUGET_PLUGIN_PATHS: undefined, NUGET_CREDENTIALPROVIDERS_PATH: undefined };
  return {
    ...removed, DOTNET_ROOT: path.dirname(command), DOTNET_MULTILEVEL_LOOKUP: '0',
    DOTNET_CLI_HOME: path.join(scratch, 'cli-home'), DOTNET_CLI_TELEMETRY_OPTOUT: '1',
    DOTNET_SKIP_FIRST_TIME_EXPERIENCE: '1', DOTNET_GENERATE_ASPNET_CERTIFICATE: 'false',
    DOTNET_CLI_WORKLOAD_UPDATE_NOTIFY_DISABLE: 'true',
    TMP: path.join(projectRoot, 'tmp'), TEMP: path.join(projectRoot, 'tmp'), TMPDIR: path.join(projectRoot, 'tmp'),
    NUGET_PACKAGES: path.join(projectRoot, 'tmp', 'build-cache', 'nuget'),
  };
}
export function assertSdk(command = dotnetCommand()) {
  const result = spawnSync(command, ['--version'], { cwd: projectRoot, env: { ...process.env, ...dotnetEnvironment(command) }, encoding: 'utf8', timeout: 15000, windowsHide: true });
  if (result.error || result.status !== 0 || result.stdout.trim() !== sdkVersion)
    throw new Error(`Active helper requires SDK ${sdkVersion}: ${result.error?.message || result.stdout || result.stderr}`);
  return command;
}
export function releaseFile(metadata, arch, now = new Date()) {
  if (!['x64', 'arm64'].includes(arch) || metadata['channel-version'] !== channel || !['active', 'maintenance'].includes(metadata['support-phase']) ||
      !/^\d{4}-\d{2}-\d{2}$/.test(metadata['eol-date'] ?? '') || !Number.isFinite(new Date(metadata['eol-date']).valueOf()) || new Date(metadata['eol-date']) <= now || metadata['latest-sdk'] !== sdkVersion || metadata['latest-runtime'] !== runtimeVersion)
    throw new Error('Active helper SDK/runtime pins or supported channel lifecycle need review.');
  const release = metadata.releases.find(value => value.sdk?.version === sdkVersion && value.runtime?.version === runtimeVersion);
  const file = release?.sdk.files.find(value => value.rid === `win-${arch}` && value.name === `dotnet-sdk-win-${arch}.zip`);
  if (!file || !/^[a-f0-9]{128}$/i.test(file.hash) ||
      file.url !== `https://builds.dotnet.microsoft.com/dotnet/Sdk/${sdkVersion}/dotnet-sdk-${sdkVersion}-win-${arch}.zip`)
    throw new Error('Official SDK archive identity/hash is missing.');
  return file;
}
export function windowsSection(document) {
  const match = /## Windows\r?\n([\s\S]*?)(?=\n\[0\]:|\n## )/.exec(document);
  if (!match || !match[1].includes('10 1809') || !match[1].includes('Arm64, x64')) throw new Error('Windows support evidence is incomplete.');
  return match[1].trim();
}
async function hashFile(file) {
  const hash = createHash('sha512');
  for await (const bytes of createReadStream(file)) hash.update(bytes);
  return hash.digest('hex');
}
async function getText(url) {
  const response = await fetch(url, { signal: AbortSignal.timeout(60000) });
  if (!response.ok) throw new Error(`Official metadata fetch failed: ${response.status} ${url}`);
  const chunks = []; let size = 0;
  for await (const chunk of response.body) {
    size += chunk.length;
    if (size > 4 * 1024 * 1024) throw new Error('Official metadata exceeds its size bound.');
    chunks.push(Buffer.from(chunk));
  }
  return Buffer.concat(chunks).toString('utf8');
}
export async function retainRuntimeNotices(directory) {
  const files = [];
  for (const [name, sha256] of runtimeNotices) {
    const url = `https://raw.githubusercontent.com/dotnet/runtime/v${runtimeVersion}/${name}`;
    const bytes = Buffer.from(await getText(url));
    if (createHash('sha256').update(bytes).digest('hex') !== sha256) throw new Error(`.NET runtime notice checksum mismatch: ${name}`);
    writeFileSync(path.join(directory, `DOTNET-${name}`), bytes, { flag: 'wx' });
    files.push({ url, sha256 });
  }
  writeFileSync(path.join(directory, 'notice-provenance.json'), JSON.stringify({
    runtime: runtimeVersion, files, authenticatedBy: 'Pinned SHA-256, HTTPS version tag; not a detached signature',
  }, null, 2) + '\n', { flag: 'wx' });
}
export async function provision(arch = process.arch) {
  if (process.platform !== 'win32') throw new Error('Active Windows SDK provisioning requires a Windows build host.');
  const evidence = path.join(scratch, 'evidence'); mkdirSync(evidence, { recursive: true });
  const raw = await getText(metadataUrl); const metadata = JSON.parse(raw); const file = releaseFile(metadata, arch);
  writeFileSync(path.join(evidence, '10.0-releases.json'), raw);
  const previousRaw = await getText('https://builds.dotnet.microsoft.com/dotnet/release-metadata/8.0/releases.json');
  writeFileSync(path.join(evidence, '8.0-releases.json'), previousRaw);
  const previous = JSON.parse(previousRaw);
  const documents = {};
  for (const version of ['8.0', channel]) {
    const url = `https://raw.githubusercontent.com/dotnet/core/main/release-notes/${version}/supported-os.md`;
    documents[version] = await getText(url);
    writeFileSync(path.join(evidence, `${version}-supported-os.md`), documents[version]);
  }
  if (windowsSection(documents['8.0']) !== windowsSection(documents[channel]))
    throw new Error('Official Windows support matrices differ. Review OS coverage before changing the active helper.');
  const archive = path.join(scratch, `dotnet-sdk-${sdkVersion}-win-${arch}.zip`);
  if (!existsSync(archive)) {
    const partial = archive + `.${process.pid}.partial`;
    try {
      const response = await fetch(file.url, { signal: AbortSignal.timeout(240000) });
      if (!response.ok || !response.body) throw new Error(`Official SDK download failed: ${response.status}`);
      await pipeline(Readable.fromWeb(response.body), createWriteStream(partial, { flags: 'wx' }));
      if (await hashFile(partial) !== file.hash.toLowerCase()) throw new Error('Official SDK SHA-512 mismatch.');
      renameSync(partial, archive);
    } finally { rmSync(partial, { force: true }); }
  }
  if (await hashFile(archive) !== file.hash.toLowerCase()) throw new Error('Cached SDK SHA-512 mismatch. Nothing extracted or executed.');
  const destination = sdkDirectory(arch); const marker = path.join(destination, '.shellfox-sdk.json');
  if (existsSync(destination)) {
    if (!existsSync(marker) || JSON.parse(readFileSync(marker, 'utf8')).sdk !== sdkVersion)
      throw new Error('Refusing to replace an unowned SDK directory.');
    rmSync(destination, { recursive: true });
  }
  mkdirSync(destination, { recursive: true });
  try {
    const tar = path.join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'tar.exe');
    const result = spawnSync(tar, ['-xf', archive, '-C', destination], { timeout: 240000, windowsHide: true, encoding: 'utf8' });
    if (result.error || result.status !== 0) throw new Error(`SDK extraction failed: ${result.error?.message || result.stderr}`);
    const command = path.join(destination, 'dotnet.exe'); assertSdk(command);
    const record = {
      sdk: sdkVersion, runtime: runtimeVersion, rid: `win-${arch}`, source: metadataUrl,
      archive: file.url, sha512: file.hash.toLowerCase(), supportPhase: metadata['support-phase'], eol: metadata['eol-date'],
      metadataSha256: createHash('sha256').update(raw).digest('hex'),
      retiredChannel: { channel: previous['channel-version'], supportPhase: previous['support-phase'], latestRuntime: previous['latest-runtime'], eol: previous['eol-date'], shipped: false },
      windowsDocuments: Object.entries(documents).map(([version, text]) => ({
        url: `https://raw.githubusercontent.com/dotnet/core/main/release-notes/${version}/supported-os.md`, sha256: createHash('sha256').update(text).digest('hex'),
      })),
      windowsSectionsEqual: true, helperMinimumWindowsBuild: 17763,
      checkedAt: new Date().toISOString(), authentication: 'Official Microsoft HTTPS release metadata and matching archive SHA-512',
    };
    writeFileSync(marker, JSON.stringify(record, null, 2) + '\n');
    writeFileSync(path.join(evidence, `sdk-win-${arch}.json`), JSON.stringify(record, null, 2) + '\n');
    console.log(JSON.stringify({ command, ...record }, null, 2));
    return command;
  } catch (error) { rmSync(destination, { recursive: true, force: true }); throw error; }
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const command = process.argv.slice(2).join(' ');
  if (command === 'provision') await provision();
  else if (command === 'test') {
    const sdk = assertSdk();
    const result = spawnSync(sdk, ['test', path.join(projectRoot, 'native/Shellfox.ElevatedTerminal.Tests/Shellfox.ElevatedTerminal.Tests.csproj'),
      '-c', 'Release', '--logger', 'trx;LogFileName=active-helper.trx', '--results-directory', path.join(projectRoot, 'tmp/reports/net10-helper')],
    { cwd: projectRoot, env: { ...process.env, ...dotnetEnvironment(sdk) }, stdio: 'inherit', timeout: 180000, windowsHide: true });
    if (result.error || result.status !== 0) throw result.error || new Error(`Active helper tests failed: ${result.status}`);
  } else throw new Error('Expected provision or test.');
}
