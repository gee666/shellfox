import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { treePackages } from './rust-package-audit.mjs';
import { compilerHost, fileHash, nativeTarget, root, run, scratchRoot, sha256, targets } from './rust-package-common.mjs';

export const auditVersion = '0.22.2';
export const advisoryDbCommit = '4ef2df8e806a65e2f1ecc14f56f51249062b23d6';
const database = path.join(scratchRoot, 'rust-package-advisory-db');
const databaseUrl = 'https://github.com/RustSec/advisory-db.git';

export function fetchAdvisories() {
  mkdirSync(scratchRoot, { recursive: true });
  if (!existsSync(database)) {
    run('git', ['init', database]);
    run('git', ['-C', database, 'fetch', '--depth=1', databaseUrl, advisoryDbCommit]);
    run('git', ['-C', database, 'checkout', '--detach', advisoryDbCommit]);
  }
  const actual = run('git', ['-C', database, 'rev-parse', 'HEAD']).trim();
  if (actual !== advisoryDbCommit) throw new Error(`Advisory DB commit mismatch: ${actual}`);
  return database;
}

export function assertAdvisoryReport(report) {
  if (!report || report.error || !Array.isArray(report.vulnerabilities?.list) ||
      report.vulnerabilities.count !== report.vulnerabilities.list.length ||
      report.vulnerabilities.found !== (report.vulnerabilities.count > 0) ||
      !Number.isInteger(report.database?.['advisory-count']) || report.database['advisory-count'] <= 0 ||
      !Number.isInteger(report.lockfile?.['dependency-count']) ||
      !Array.isArray(report.settings?.ignore) || report.settings.ignore.length || report.settings.severity !== null ||
      !report.warnings || Object.values(report.warnings).some(items => !Array.isArray(items))) {
    throw new Error('Incomplete, filtered or ignored-advisory Cargo audit report; refusing a passing result');
  }
}

export function assertDatabaseAge(timestamp, now = Date.now()) {
  const age = now - timestamp * 1000;
  if (!Number.isFinite(age) || age < -24 * 60 * 60_000 || age > 90 * 24 * 60 * 60_000) {
    throw new Error('Pinned advisory DB is stale or has an invalid future timestamp; review a fresh pin');
  }
}

const targetOs = triple => triple.includes('windows') ? 'windows' : triple.includes('darwin') ? 'macos' : 'linux';
const targetArch = triple => triple.startsWith('aarch64') ? 'aarch64' : 'x86_64';

export function classifyAdvisories(report, active) {
  const classify = item => ({
    id: item.advisory.id, package: item.package.name, version: item.package.version,
    title: item.advisory.title, url: item.advisory.url,
    activeInSelectedTree: active.has(`${item.package.name}@${item.package.version}`),
  });
  return {
    vulnerabilities: (report.vulnerabilities?.list ?? []).map(classify),
    warnings: Object.entries(report.warnings ?? {}).flatMap(([kind, items]) => items.map(item => ({ kind, ...classify(item) }))),
  };
}

export function auditSecurity({ packages, target, output }) {
  if (!new RegExp(`\\b${auditVersion.replaceAll('.', '\\.')}\\b`).test(run('cargo', ['audit', '--version']))) {
    throw new Error(`Install project audit tool explicitly: cargo install cargo-audit --locked --version ${auditVersion}`);
  }
  if (!existsSync(database)) throw new Error('No local advisory database. Explicitly run node scripts/rust-package-security.mjs fetch-db');
  if (run('git', ['-C', database, 'rev-parse', 'HEAD']).trim() !== advisoryDbCommit ||
      run('git', ['-C', database, 'status', '--porcelain']).trim()) throw new Error('Advisory database is changed or at the wrong commit');
  assertDatabaseAge(Number(run('git', ['-C', database, 'show', '-s', '--format=%ct', 'HEAD']).trim()));
  const buildHost = compilerHost();
  const treeArgs = ['tree', '--locked', '--target', target, '--edges', 'normal,build', '--prefix', 'none', '--format', '{p}'];
  for (const pkg of packages) treeArgs.push('-p', pkg);
  const active = treePackages(run('cargo', treeArgs));
  const operatingSystems = [...new Set([targetOs(target), targetOs(buildHost)])];
  const architectures = [...new Set([targetArch(target), targetArch(buildHost)])];
  const result = spawnSync('cargo', ['audit', '--json', '--no-fetch', '--no-yanked', '--db', database,
    '--file', path.join(root, 'Cargo.lock'), ...operatingSystems.flatMap(os => ['--target-os', os]),
    ...architectures.flatMap(arch => ['--target-arch', arch])], {
    cwd: root, timeout: 120_000, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024, windowsHide: true,
    env: { ...process.env, TMP: scratchRoot, TEMP: scratchRoot, TMPDIR: scratchRoot },
  });
  if (result.error || ![0, 1].includes(result.status)) throw new Error(`Cargo advisory audit unavailable: ${result.error?.message ?? result.stderr}`);
  let raw;
  try { raw = JSON.parse(result.stdout); } catch { throw new Error(`Cargo advisory report unavailable: ${result.stderr}`); }
  mkdirSync(output, { recursive: true });
  writeFileSync(path.join(output, 'security-audit-raw.json'), JSON.stringify(raw, null, 2) + '\n');
  assertAdvisoryReport(raw);
  const sameValues = (actual, expected) => Array.isArray(actual) && [...actual].sort().join(',') === [...expected].sort().join(',');
  if (!sameValues(raw.settings.target_os, operatingSystems) || !sameValues(raw.settings.target_arch, architectures)) {
    throw new Error('Cargo audit did not use the requested target and build-host scope');
  }
  const findings = classifyAdvisories(raw, active);
  const report = {
    format: 'shellfox-rust-security-audit-v1', tool: `cargo-audit ${auditVersion}`, advisoryDbCommit,
    cargoLockSha256: fileHash(path.join(root, 'Cargo.lock')), target, buildHost, packages, operatingSystems, architectures,
    edges: ['normal', 'build'], selectedTreeSha256: sha256([...active].sort().join('\n')),
    policy: { activeVulnerabilities: 'deny', inactiveLockfileFindings: 'retain, not a waiver', maintenanceWarnings: 'report' },
    yankedCheck: false, legalApproval: false,
    ...findings,
    passed: !findings.vulnerabilities.some(item => item.activeInSelectedTree),
  };
  writeFileSync(path.join(output, 'security-audit.json'), JSON.stringify(report, null, 2) + '\n');
  if (!report.passed) throw new Error('Active selected dependency has a RustSec vulnerability. See security-audit.json; no advisory was ignored.');
  return report;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command, kind, output, requestedTarget, ...extra] = process.argv.slice(2);
  if (command === 'fetch-db' && !kind && !output && !requestedTarget && !extra.length) console.log(fetchAdvisories());
  else if (command === 'audit' && ['desktop', 'headless'].includes(kind) && output && !extra.length) {
    const target = requestedTarget ?? nativeTarget();
    if (!Object.values(targets).includes(target)) throw new Error('Expected one of the six portable Rust target triples');
    console.log(JSON.stringify(auditSecurity({ target, packages: kind === 'headless' ? ['shellfox-cli'] : ['shellfox-desktop', 'shellfox-cli'], output: path.resolve(output) }), null, 2));
  } else throw new Error('Usage: node scripts/rust-package-security.mjs fetch-db | audit desktop|headless OUTPUT [RUST_TARGET]');
}
