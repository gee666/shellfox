import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { assertHeadlessNames, compilerHost, fileHash, root, run, sha256 } from './rust-package-common.mjs';
import { supplementalLicense } from './rust-package-licenses.mjs';

export function lockPackages(lock) {
  return lock.split(/^\[\[package\]\]\s*$/m).slice(1).map(block => {
    const result = {};
    for (const key of ['name', 'version', 'source', 'checksum']) {
      const value = new RegExp(`^${key} = ("[^"\\n]*")$`, 'm').exec(block)?.[1];
      if (value) result[key] = JSON.parse(value);
    }
    if (!result.name || !result.version) throw new Error('Invalid Cargo.lock package record');
    return result;
  });
}

export function treePackages(tree) {
  const result = new Set();
  for (const line of tree.trim().split(/\r?\n/)) {
    if (!line.trim()) continue;
    const match = /^([\w-]+) v([^\s]+)(?:\s|$)/.exec(line);
    if (!match) throw new Error(`Unexpected cargo tree record: ${line}`);
    result.add(`${match[1]}@${match[2]}`);
  }
  return result;
}

function licenseFiles(directory) {
  const names = readdirSync(directory, { withFileTypes: true });
  const result = names.filter(entry => entry.isFile() && /^(?:licen[sc]e|copying|notice|copyright)(?:[._-].*)?$/i.test(entry.name))
    .map(entry => path.join(directory, entry.name));
  for (const entry of names.filter(entry => entry.isDirectory() && /^(?:licenses?|licences?)$/i.test(entry.name))) {
    for (const file of readdirSync(path.join(directory, entry.name), { withFileTypes: true })) {
      if (file.isFile()) result.push(path.join(directory, entry.name, file.name));
    }
  }
  return result;
}

export function assertLicenseTexts(manifest) {
  const missing = manifest.dependencies.filter(pkg => pkg.reviewRequired);
  if (missing.length) throw new Error(`License review required: ${missing.map(pkg => `${pkg.name}@${pkg.version}`).join(', ')}`);
}

export function auditDependencies({ packages, target, output, headless = false, strict = false }) {
  // A filtered metadata result can omit native build-host dependencies during
  // cross-target audits. The target-specific tree below selects the real graph.
  const metadata = JSON.parse(run('cargo', ['metadata', '--locked', '--format-version', '1']));
  const args = ['tree', '--locked', '--target', target, '--edges', 'normal,build', '--prefix', 'none', '--format', '{p}'];
  for (const pkg of packages) args.push('-p', pkg);
  const tree = run('cargo', args);
  const selected = treePackages(tree);
  if (headless) assertHeadlessNames([...selected].map(key => key.split('@')[0]));
  const lock = readFileSync(path.join(root, 'Cargo.lock'), 'utf8');
  const locked = lockPackages(lock);
  const notices = path.join(output, 'notices', 'cargo');
  mkdirSync(notices, { recursive: true });
  writeFileSync(path.join(output, 'cargo-tree.txt'), tree);
  const matched = metadata.packages.filter(pkg => selected.has(`${pkg.name}@${pkg.version}`));
  if (new Set(matched.map(pkg => `${pkg.name}@${pkg.version}`)).size !== matched.length) {
    throw new Error('Selected dependency name/version is ambiguous across sources');
  }
  const dependencies = matched.map(pkg => {
    const record = locked.find(entry => entry.name === pkg.name && entry.version === pkg.version && (entry.source ?? null) === pkg.source);
    if (!record) throw new Error(`Cargo.lock has no matching package: ${pkg.id}`);
    const directory = path.dirname(pkg.manifest_path);
    const candidates = licenseFiles(directory);
    if (pkg.license_file) {
      const file = path.resolve(directory, pkg.license_file);
      if (existsSync(file)) candidates.push(file);
    }
    // Workspace crates inherit the repository MIT license, not third-party crates.
    if (metadata.workspace_members.includes(pkg.id) && pkg.license === 'MIT') candidates.push(path.join(root, 'LICENSE'));
    const destination = path.join(notices, `${pkg.name}-${pkg.version}`);
    mkdirSync(destination, { recursive: true });
    const files = [...new Set(candidates)].map((file, index) => {
      const name = `${index}-${path.basename(file)}`;
      copyFileSync(file, path.join(destination, name));
      return { path: `notices/cargo/${pkg.name}-${pkg.version}/${name}`, sha256: fileHash(file), role: 'license-text' };
    });
    const supplemental = supplementalLicense(pkg, record, destination);
    for (const file of supplemental?.files ?? []) files.push({
      path: `notices/cargo/${pkg.name}-${pkg.version}/${file.name}`, sha256: file.sha256, role: file.role,
    });
    return {
      name: pkg.name, version: pkg.version, source: pkg.source, checksum: record.checksum ?? null,
      license: pkg.license, repository: pkg.repository, licenseFiles: files,
      selectedLicense: supplemental?.selectedLicense ?? null,
      unresolvedReason: supplemental?.unresolvedReason ?? null,
      reviewRequired: !pkg.license || !files.some(file => file.role === 'license-text'),
    };
  }).sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`));
  if (new Set(dependencies.map(pkg => `${pkg.name}@${pkg.version}`)).size !== selected.size) {
    throw new Error('Cargo metadata did not cover the selected dependency tree');
  }
  const manifest = {
    format: 'shellfox-rust-license-audit-v1', target, buildHost: compilerHost(), packages,
    cargoLockSha256: sha256(lock), edges: ['normal', 'build'],
    legalApproval: false, dependencies,
  };
  writeFileSync(path.join(output, 'dependency-licenses.json'), JSON.stringify(manifest, null, 2) + '\n');
  const missing = dependencies.filter(pkg => pkg.reviewRequired);
  writeFileSync(path.join(output, 'THIRD-PARTY-NOTICES.txt'),
    'Rust dependency license expressions come from locked Cargo metadata. This is an inventory, not legal approval.\n' +
    'Build dependencies are included conservatively. Go and Ghostty have separate retained notices.\n\n' +
    dependencies.map(pkg => `${pkg.name} ${pkg.version}\nSource: ${pkg.source ?? 'workspace/path'}\nLicense: ${pkg.license ?? 'UNKNOWN'}\n` +
      pkg.licenseFiles.map(file => `Text: ${file.path}\n`).join('') + (pkg.reviewRequired ? 'REVIEW REQUIRED: license expression or text missing\n' : '')).join('\n'));
  console.log(`[rust-package] ${dependencies.length} Cargo dependencies; ${missing.length} require license review`);
  if (strict) assertLicenseTexts(manifest);
  return manifest;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [target, packageName, output, flag, ...extra] = process.argv.slice(2);
  if (!target || !packageName || !output || (flag && flag !== '--strict') || extra.length) {
    throw new Error('Usage: node scripts/rust-package-audit.mjs TARGET PACKAGE OUTPUT [--strict]');
  }
  auditDependencies({ target, packages: [packageName], output: path.resolve(output), headless: packageName !== 'shellfox-desktop', strict: flag === '--strict' });
}
