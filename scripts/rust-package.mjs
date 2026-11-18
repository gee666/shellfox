import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { parseArgs } from 'node:util';
import { fileURLToPath } from 'node:url';
import { auditDependencies } from './rust-package-audit.mjs';
import { stageResources, writeLaunchers } from './rust-package-resources.mjs';
import { assertHeadlessFiles, executableName, fileHash, filesUnder, nativeTarget, root, run, scratchRoot, selectedBinaries, sha256 } from './rust-package-common.mjs';
import { smokePackage } from './rust-package-smoke.mjs';
import { auditSecurity } from './rust-package-security.mjs';

export function writeManifest(directory, details, anchorFile = null) {
  if (anchorFile !== null) {
    const relative = path.relative(directory, anchorFile);
    if (!path.isAbsolute(anchorFile) || (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative))) {
      throw new Error('Trusted manifest anchor must be an absolute path outside the payload directory');
    }
    if (existsSync(anchorFile)) throw new Error('Refusing to overwrite an existing trusted manifest anchor');
  }
  const files = filesUnder(directory).filter(file => file !== 'package-manifest.json').map(file => ({
    path: file, sha256: fileHash(path.join(directory, file)),
  }));
  const manifest = {
    format: 'shellfox-rust-portable-v1', ...details,
    resourceRoot: 'resources', dataSelection: { argument: '--data-dir', absoluteRequired: true, productionDefault: false },
    update: { supported: false, reason: 'Experimental portable Rust archive. Existing Electron/Squirrel/DEB/app-bundle and Node-headless update channels do not accept this format.' },
    signing: 'not performed by packaging', files,
  };
  const bytes = Buffer.from(JSON.stringify(manifest, null, 2) + '\n');
  writeFileSync(path.join(directory, 'package-manifest.json'), bytes);
  const expectedManifestDigest = sha256(bytes);
  if (anchorFile !== null) {
    writeFileSync(anchorFile, `${expectedManifestDigest}  package-manifest.json\n`, { flag: 'wx' });
  }
  // Trusted BUILD output, not a runtime instruction to self-hash the manifest.
  return { manifest, expectedManifestDigest };
}

export async function packageRust(kind) {
  if (!['desktop', 'headless'].includes(kind)) throw new Error('Expected desktop or headless');
  const target = nativeTarget();
  const metadata = JSON.parse(run('cargo', ['metadata', '--locked', '--no-deps', '--format-version', '1']));
  const binaries = selectedBinaries(metadata, kind);
  const version = binaries[0].version;
  if (!/^[0-9A-Za-z.+-]+$/.test(version)) throw new Error('Unsafe Cargo version in artifact name');
  const name = `shellfox-rust-${kind}-${version}-${process.platform}-${process.arch}`;
  const work = mkdtempSync(path.join(scratchRoot, 'rust-package-build-'));
  const artifacts = path.join(scratchRoot, 'rust-package-artifacts');
  mkdirSync(artifacts, { recursive: true });
  const archive = path.join(artifacts, name + (process.platform === 'win32' ? '.zip' : '.tar.gz'));
  if (existsSync(archive) || existsSync(archive + '.sha256') || existsSync(archive + '.package-manifest.sha256')) {
    rmSync(work, { recursive: true, force: true });
    throw new Error(`Artifact already exists; move or remove only this experiment's old output first: ${archive}`);
  }
  const auditDirectory = path.join(work, 'audit');
  let complete = false;
  let preserveScratch = false;
  try {
    const directory = path.join(work, name);
    mkdirSync(path.join(directory, 'bin'), { recursive: true });
    const packages = binaries.map(bin => bin.packageName);
    const audit = auditDependencies({ packages, target, output: auditDirectory, headless: kind === 'headless', strict: true });
    const security = auditSecurity({ packages, target, output: auditDirectory });
    const assertLockUnchanged = () => {
      if (security.cargoLockSha256 !== audit.cargoLockSha256 || fileHash(path.join(root, 'Cargo.lock')) !== audit.cargoLockSha256) {
        throw new Error('Cargo.lock changed during packaging; rerun audits against one stable source state');
      }
    };
    assertLockUnchanged();
    cpSync(auditDirectory, directory, { recursive: true });
    // Native manifests and trusted-build anchors must exist before Rust compiles.
    const { records: resources, anchorEnv } = await stageResources({ directory, work, kind, version,
      nugetSnapshot: process.env.SHELLFOX_NUGET_VULNERABILITY_SNAPSHOT ?? null });
    assertLockUnchanged();
    // Never overwrite target/release: a user-owned desktop may be loading its DLL.
    // Native-only builds omit the redundant target-triple directory. MSVC's
    // header loader still fails on long paths inside Ghostty's nested prefix.
    const cargoTarget = path.join(scratchRoot, `rp-${process.platform}-${process.arch}`);
    const buildArgs = ['build', '--locked', '--release', '--target-dir', cargoTarget];
    for (const { packageName, bin } of binaries) buildArgs.push('-p', packageName, '--bin', bin);
    run('cargo', buildArgs, { env: anchorEnv });
    assertLockUnchanged();
    const profileDirectory = path.join(cargoTarget, 'release');
    for (const { bin } of binaries) {
      const destination = path.join(directory, 'bin', executableName(bin));
      copyFileSync(path.join(profileDirectory, executableName(bin)), destination);
      chmodSync(destination, 0o755);
    }
    if (process.platform === 'win32') {
      copyFileSync(path.join(profileDirectory, 'ghostty-vt.dll'), path.join(directory, 'bin', 'ghostty-vt.dll'));
      resources.push({ name: 'ghostty-vt', path: 'bin/ghostty-vt.dll', use: 'loader-adjacent runtime dependency' });
    }
    writeLaunchers(directory, kind);
    copyFileSync(path.join(root, 'LICENSE'), path.join(directory, 'LICENSE'));
    writeFileSync(path.join(directory, 'README.txt'),
      `Experimental Shellfox Rust ${kind} package. Not a release or installer.\n` +
      'Pass --data-dir with an absolute NEW experiment-only directory. Never select installed Shellfox data.\n' +
      'CLI global flags --data-dir ABS --helper ABS --helper-sha256 TRUSTED_MANIFEST_DIGEST must precede the command.\n' +
      'Use the packaged resources/tailscale/shellfox-tailscale executable and its file digest from the trusted package-manifest.json.\n' +
      'No environment-based resource or user-data selection is promised. See package-manifest.json and dependency-licenses.json.\n' +
      'Packaged resources do not prove Rust runtime adapter parity. Update, signing, startup registration and full platform acceptance remain separate gates.\n');
    assertLockUnchanged();
    const { manifest } = writeManifest(directory, {
      kind, version, target, platform: process.platform, arch: process.arch,
      binaries: binaries.map(({ bin, packageName }) => ({ package: packageName, path: `bin/${executableName(bin)}` })),
      resources, cargoLockSha256: audit.cargoLockSha256,
      securityAudit: { passed: security.passed, advisoryDbCommit: security.advisoryDbCommit, tool: security.tool,
        coverage: 'Cargo selected tree only. Managed/NuGet reports are separate native bundle files; no all-native-clean claim.' },
      licenseReviewRequired: audit.dependencies.filter(pkg => pkg.reviewRequired).map(pkg => `${pkg.name}@${pkg.version}`),
    }, archive + '.package-manifest.sha256');
    if (kind === 'headless') assertHeadlessFiles(manifest.files.map(file => file.path));
    if (process.platform === 'win32') {
      const quote = value => "'" + value.replaceAll("'", "''") + "'";
      run(path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/WindowsPowerShell/v1.0/powershell.exe'),
        ['-NoProfile', '-NonInteractive', '-Command', `$ErrorActionPreference='Stop'; Compress-Archive -LiteralPath ${quote(directory)} -DestinationPath ${quote(archive)} -CompressionLevel Optimal`]);
    } else run('tar', ['-czf', archive, '-C', work, name]);
    writeFileSync(archive + '.sha256', `${fileHash(archive)}  ${path.basename(archive)}\n`);
    const unpack = path.join(work, 'archive smoke with spaces');
    mkdirSync(unpack);
    const tar = process.platform === 'win32' ? path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/tar.exe') : 'tar';
    run(tar, ['-xf', archive, '-C', unpack]);
    const evidence = await smokePackage(path.join(unpack, name));
    writeFileSync(archive + '.acceptance.json', JSON.stringify(evidence, null, 2) + '\n');
    complete = true;
    console.log(`[rust-package] ${archive}`);
    return archive;
  } catch (error) {
    preserveScratch = error.preserveScratch === true;
    if (existsSync(auditDirectory)) {
      const reports = path.join(scratchRoot, 'rust-package-reports', name);
      mkdirSync(reports, { recursive: true });
      cpSync(auditDirectory, reports, { recursive: true });
      console.error(`[rust-package] Failed audit evidence retained: ${reports}`);
    }
    if (preserveScratch) console.error(`[rust-package] Package scratch retained for owned host cleanup: ${work}`);
    throw error;
  } finally {
    if (!complete) for (const suffix of ['', '.sha256', '.package-manifest.sha256', '.acceptance.json']) rmSync(archive + suffix, { force: true });
    if (!preserveScratch) rmSync(work, { recursive: true, force: true });
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { positionals } = parseArgs({ allowPositionals: true, options: {} });
  if (positionals.length !== 1) throw new Error('Usage: node scripts/rust-package.mjs desktop|headless');
  await packageRust(positionals[0]);
}
