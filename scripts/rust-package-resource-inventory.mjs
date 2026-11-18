import { lstatSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileHash, readJson, sha256 } from './rust-package-common.mjs';

const maxJson = 4 * 1024 * 1024;
function boundedJson(file) {
  const info = lstatSync(file);
  if (!info.isFile() || info.isSymbolicLink() || info.nlink !== 1 || info.size > maxJson) throw new Error(`Managed metadata file/alias bound: ${file}`);
  const bytes = readFileSync(file);
  if (bytes.length > maxJson) throw new Error(`Managed metadata exceeds bound: ${file}`);
  return JSON.parse(bytes.toString('utf8'));
}

export function managedInventory(directory, { rid, runtimeVersion, assembly = 'Shellfox.ElevatedTerminal' }) {
  const depsFile = path.join(directory, `${assembly}.deps.json`);
  const configFile = path.join(directory, `${assembly}.runtimeconfig.json`);
  const deps = boundedJson(depsFile), config = boundedJson(configFile);
  if (!/^\d+\.\d+\.\d+$/.test(runtimeVersion)) throw new Error('Exact managed runtime version required');
  const framework = runtimeVersion.split('.').slice(0, 2).join('.');
  const target = `.NETCoreApp,Version=v${framework}/${rid}`;
  const frameworks = config.runtimeOptions?.includedFrameworks;
  if (deps.runtimeTarget?.name !== target || config.runtimeOptions?.tfm !== `net${framework}` ||
      !Array.isArray(frameworks) || frameworks.length !== 1 || frameworks[0].name !== 'Microsoft.NETCore.App' ||
      frameworks[0].version !== runtimeVersion || Object.hasOwn(config.runtimeOptions, 'framework') || Object.hasOwn(config.runtimeOptions, 'frameworks') || Object.hasOwn(config.runtimeOptions, 'additionalProbingPaths')) {
    throw new Error('Published RID or self-contained runtime differs from declared trusted build');
  }
  const packages = Object.entries(deps.libraries ?? {}).map(([identity, value]) => {
    const split = identity.lastIndexOf('/');
    if (split < 1 || !['project', 'package', 'runtimepack', 'reference'].includes(value.type)) throw new Error(`Unknown managed library record: ${identity}`);
    const rawName = identity.slice(0, split);
    return { name: rawName.replace(/^runtimepack\./, ''), version: identity.slice(split + 1), kind: value.type,
      nugetSha512: value.sha512 ?? null };
  });
  if (!packages.some(pkg => pkg.kind === 'runtimepack' && pkg.name === `Microsoft.NETCore.App.Runtime.${rid}` && pkg.version === runtimeVersion)) throw new Error('Self-contained runtime version inventory missing');
  const libraries = deps.targets?.[target];
  if (!libraries || !Object.hasOwn(libraries, `runtimepack.Microsoft.NETCore.App.Runtime.${rid}/${runtimeVersion}`)) throw new Error('Published runtime pack missing');
  const assets = [];
  for (const [library, value] of Object.entries(libraries)) for (const category of ['runtime', 'native', 'resources', 'runtimeTargets']) {
    for (const [name, versions] of Object.entries(value[category] ?? {})) {
      if (name.endsWith('/_._')) continue;
      if (name.includes('\\') || name.split('/').some(part => !part || part === '.' || part === '..') || path.isAbsolute(name)) throw new Error('Unsafe managed asset path');
      assets.push({ library, category, path: name, assemblyVersion: versions.assemblyVersion ?? null, fileVersion: versions.fileVersion ?? null });
    }
  }
  const inventory = { format: 'shellfox-managed-inventory-v1', rid, runtimeVersion, selfContained: true,
    metadata: [{ path: `${assembly}.deps.json`, sha256: fileHash(depsFile) }, { path: `${assembly}.runtimeconfig.json`, sha256: fileHash(configFile) }],
    packages, assets, coverage: 'Published .deps/runtimeconfig inventory. Not an OS, Cargo, Go or complete native-code security audit.' };
  writeFileSync(path.join(directory, 'managed-inventory.json'), JSON.stringify(inventory, null, 2) + '\n', { flag: 'wx' });
  return inventory;
}

function version(value) {
  if (typeof value !== 'string' || value.length > 128) return null;
  const match = /^(\d+(?:\.\d+){0,3})(?:-([A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*))?(?:\+[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*)?$/.exec(value);
  if (!match) return null;
  const numbers = match[1].split('.').map(Number);
  if (numbers.some(value => !Number.isSafeInteger(value) || value > 2147483647)) return null;
  return { numbers: numbers.concat([0, 0, 0, 0]).slice(0, 4), prerelease: match[2]?.toLowerCase().split('.') ?? [] };
}
function compare(a, b) {
  for (let i = 0; i < 4; i++) if (a.numbers[i] !== b.numbers[i]) return Math.sign(a.numbers[i] - b.numbers[i]);
  if (!a.prerelease.length || !b.prerelease.length) return Math.sign(b.prerelease.length) - Math.sign(a.prerelease.length);
  for (let i = 0; i < Math.max(a.prerelease.length, b.prerelease.length); i++) {
    const left = a.prerelease[i], right = b.prerelease[i];
    if (left === undefined || right === undefined) return left === undefined ? -1 : 1;
    if (left === right) continue;
    const ln = /^\d+$/.test(left), rn = /^\d+$/.test(right);
    if (ln && rn) {
      if (BigInt(left) !== BigInt(right)) return BigInt(left) < BigInt(right) ? -1 : 1;
      continue;
    }
    if (ln !== rn) return ln ? -1 : 1;
    return left < right ? -1 : 1;
  }
  return 0;
}
export function nugetRangeContains(release, range) {
  const actual = version(release);
  if (!actual || typeof range !== 'string' || range.length > 256) return null;
  const exact = /^\[([^,\[\]()]+)\]$/.exec(range);
  if (exact) { const expected = version(exact[1].trim()); return expected ? compare(actual, expected) === 0 : null; }
  const match = /^([\[(])\s*([^,]*)\s*,\s*([^,]*)\s*([\])])$/.exec(range);
  if (!match) return null;
  const low = match[2].trim() ? version(match[2].trim()) : null;
  const high = match[3].trim() ? version(match[3].trim()) : null;
  if ((match[2].trim() && !low) || (match[3].trim() && !high)) return null;
  if (low && (compare(actual, low) < 0 || compare(actual, low) === 0 && match[1] === '(')) return false;
  if (high && (compare(actual, high) > 0 || compare(actual, high) === 0 && match[4] === ')')) return false;
  return true;
}

export function auditManagedInventory(inventory, snapshot = null) {
  const report = { format: 'shellfox-managed-vulnerability-report-v1',
    status: snapshot ? 'no-listed-findings' : 'not-run',
    checkedPackages: [], findings: [], unresolvedRanges: [],
    sources: snapshot?.sources ?? [], runtimeLifecycle: { status: 'not-checked' },
    coverage: 'NuGet vulnerability metadata only. Does not assert .NET support status or audit runtime/OS native code, Cargo, Go, signing or publisher identity.' };
  if (!snapshot) return report;
  const lifecycle = snapshot.dotnet;
  if (lifecycle && inventory.runtimeVersion?.split('.').slice(0, 2).join('.') === lifecycle.channel) {
    if (!['active', 'maintenance', 'eol', 'preview'].includes(lifecycle.supportPhase) || !version(lifecycle.latestRuntime)) throw new Error('Invalid .NET release metadata');
    report.runtimeLifecycle = { ...lifecycle, installedRuntime: inventory.runtimeVersion,
      status: ['eol', 'preview'].includes(lifecycle.supportPhase) ? 'unsupported-channel' : compare(version(inventory.runtimeVersion), version(lifecycle.latestRuntime)) < 0 ? 'below-latest-listed' : 'current-in-listed-channel' };
  }
  if (snapshot.format !== 'shellfox-nuget-snapshot-v1' || !snapshot.packages || typeof snapshot.packages !== 'object') throw new Error('Invalid NuGet snapshot');
  for (const pkg of inventory.packages.filter(pkg => ['package', 'runtimepack'].includes(pkg.kind))) {
    report.checkedPackages.push({ name: pkg.name, version: pkg.version, kind: pkg.kind });
    const advisories = snapshot.packages[pkg.name.toLowerCase()] ?? [];
    if (!Array.isArray(advisories)) throw new Error('Invalid NuGet advisory list');
    for (const advisory of advisories) {
      if (!advisory || typeof advisory.url !== 'string' || !/^https:\/\//.test(advisory.url) || (!Number.isInteger(advisory.severity) || advisory.severity < 0 || advisory.severity > 3)) throw new Error('Invalid NuGet advisory');
      const affected = nugetRangeContains(pkg.version, advisory.versions);
      const item = { name: pkg.name, version: pkg.version, url: advisory.url, severity: advisory.severity, versions: advisory.versions };
      if (affected === null) report.unresolvedRanges.push(item);
      else if (affected) report.findings.push(item);
    }
  }
  report.status = report.findings.length ? 'findings' : report.unresolvedRanges.length ? 'incomplete' : 'no-listed-findings';
  return report;
}

async function getJson(url, maximum, expectedHost = 'api.nuget.org') {
  const parsed = new URL(url);
  if (parsed.protocol !== 'https:' || parsed.hostname !== expectedHost) throw new Error('Expected official HTTPS audit metadata URL');
  const response = await fetch(url, { signal: AbortSignal.timeout(60_000), redirect: 'error' });
  if (!response.ok) throw new Error(`NuGet metadata HTTP ${response.status}`);
  const chunks = []; let total = 0;
  for await (const chunk of response.body) {
    total += chunk.length;
    if (total > maximum) throw new Error('NuGet metadata exceeds download bound');
    chunks.push(Buffer.from(chunk));
  }
  const bytes = Buffer.concat(chunks);
  return { value: JSON.parse(bytes.toString('utf8')), source: { url, sha256: sha256(bytes) } };
}

export async function acquireNugetSnapshot(runtimeChannel = null) {
  const index = await getJson('https://api.nuget.org/v3/vulnerabilities/index.json', 1024 * 1024);
  if (!Array.isArray(index.value) || index.value.length < 1 || index.value.length > 16) throw new Error('Invalid NuGet vulnerability index');
  const sources = [index.source];
  const packages = Object.create(null);
  for (const page of index.value) {
    if (!page || typeof page['@id'] !== 'string') throw new Error('Invalid NuGet metadata page');
    const result = await getJson(page['@id'], 32 * 1024 * 1024);
    if (!result.value || typeof result.value !== 'object' || Array.isArray(result.value)) throw new Error('Invalid NuGet package page');
    sources.push({ ...result.source, updated: typeof page['@updated'] === 'string' ? page['@updated'] : null });
    for (const [name, advisories] of Object.entries(result.value)) {
      if (!Array.isArray(advisories)) throw new Error('Invalid NuGet vulnerability records');
      packages[name] = [...(packages[name] ?? []), ...advisories];
    }
  }
  let dotnet = null;
  if (runtimeChannel !== null) {
    if (!/^\d{1,2}\.\d{1,2}$/.test(runtimeChannel)) throw new Error('Explicit numeric .NET runtime channel required');
    const result = await getJson(`https://builds.dotnet.microsoft.com/dotnet/release-metadata/${runtimeChannel}/releases.json`, 4 * 1024 * 1024, 'builds.dotnet.microsoft.com');
    dotnet = { channel: result.value['channel-version'], supportPhase: result.value['support-phase'], eolDate: result.value['eol-date'],
      latestRuntime: result.value['latest-runtime'], latestReleaseDate: result.value['latest-release-date'], source: result.source };
    if (dotnet.channel !== runtimeChannel) throw new Error('.NET release metadata channel mismatch');
  }
  return { format: 'shellfox-nuget-snapshot-v1', sources, packages, dotnet };
}

export function writeManagedAudit(directory, inventory, snapshotFile = null) {
  const report = auditManagedInventory(inventory, snapshotFile ? readJson(snapshotFile) : null);
  writeFileSync(path.join(directory, 'managed-vulnerability-report.json'), JSON.stringify(report, null, 2) + '\n', { flag: 'wx' });
  return report;
}
