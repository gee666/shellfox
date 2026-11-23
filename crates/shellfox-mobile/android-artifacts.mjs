import { createHash, randomUUID } from 'node:crypto';
import { closeSync, constants, copyFileSync, existsSync, lstatSync, mkdirSync, openSync, readFileSync, readSync,
  readdirSync, renameSync, writeFileSync } from 'node:fs';
import { isAbsolute, join, relative, resolve } from 'node:path';

export const androidTargets = Object.freeze({
  'arm64-v8a': { target: 'aarch64-linux-android', zig: 'aarch64-linux-android.24', clang: 'aarch64-linux-android24-clang' },
  'armeabi-v7a': { target: 'armv7-linux-androideabi', zig: 'arm-linux-androideabi.24', clang: 'armv7a-linux-androideabi24-clang' },
  x86: { target: 'i686-linux-android', zig: 'x86-linux-android.24', clang: 'i686-linux-android24-clang' },
  x86_64: { target: 'x86_64-linux-android', zig: 'x86_64-linux-android.24', clang: 'x86_64-linux-android24-clang' },
});
export const buildSetFormat = 'shellfox-android-rust-build-set-v1';
export const buildAbiFormat = 'shellfox-android-rust-build-output-v1';
export const headerNames = ['shellfox_mobile.h', 'shellfox_mobile_client.h'];
export const defaultBuildProfile = Object.freeze({ defaultCreator: 'v1', legacyClientAbi: 1, engine: 'default-vt-1.0.1' });
export const defaultAndroidOutputRoot = project => join(project, 'tmp/mobile-rust-produced/android');
export const digest = value => typeof value === 'string' && /^[0-9a-f]{64}$/.test(value);
export function sha256(file) {
  const stat = lstatSync(file);
  if (!stat.isFile() || stat.isSymbolicLink()) throw new Error(`Expected a regular input file: ${file}`);
  const hash = createHash('sha256'), fd = openSync(file, 'r'), buffer = Buffer.alloc(65536);
  try { let count; while ((count = readSync(fd, buffer, 0, buffer.length, null))) hash.update(buffer.subarray(0, count)); }
  finally { closeSync(fd); }
  return hash.digest('hex');
}
export function readManifest(file) {
  if (lstatSync(file).size > 1024 * 1024) throw new Error(`Manifest exceeds limit: ${file}`);
  sha256(file); // Regular-file/no-link validation, shared with archive inputs.
  return JSON.parse(readFileSync(file, 'utf8'));
}
function put(file, value) { writeFileSync(file, JSON.stringify(value, null, 2) + '\n', { flag: 'wx' }); }
function inside(parent, child) {
  const r = relative(resolve(parent), resolve(child));
  return r !== '' && !isAbsolute(r) && !r.startsWith('..');
}
function sourceFiles(directory, prefix = '') {
  return readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    if (['AGENTS.md', 'auth.json', 'credentials', 'credentials.toml', '.env', 'node_modules', 'target', '.git'].includes(entry.name)) return [];
    const name = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isSymbolicLink()) throw new Error(`Source link needs explicit review: ${name}`);
    return entry.isDirectory() ? sourceFiles(join(directory, entry.name), name) : [name];
  }).sort();
}
export function fingerprintMobileSources(project) {
  const names = ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml'];
  for (const name of ['ghostty-vt-sys', 'ghostty-vt', 'shellfox-core', 'shellfox-protocol', 'shellfox-client', 'shellfox-mobile']) {
    const prefix = `crates/${name}`;
    names.push(...sourceFiles(join(project, prefix)).filter(p => !p.startsWith('build-support/vendor/')).map(p => `${prefix}/${p}`));
  }
  // Workspace manifests affect locked resolution even when their code is not built.
  for (const entry of readdirSync(join(project, 'crates'))) {
    const name = `crates/${entry}/Cargo.toml`; if (existsSync(join(project, name))) names.push(name);
  }
  const inputs = [...new Set(names)].sort().map(name => ({ path: name, sha256: sha256(join(project, name)) }));
  return createHash('sha256').update(JSON.stringify(inputs)).digest('hex');
}
export function headerHashes(project) {
  const directory = join(project, 'crates/shellfox-mobile/include');
  return Object.fromEntries(headerNames.map(name => [name, sha256(join(directory, name))]));
}
export function headerFunctions(project) {
  const directory = join(project, 'crates/shellfox-mobile/include');
  return [...new Set(headerNames.flatMap(name => [...readFileSync(join(directory, name), 'utf8')
    .matchAll(/^(?:SfmStatus|uint32_t|const char\s*\*)\s*(sfm_[a-z0-9_]+)\s*\(/gm)].map(match => match[1])))].sort();
}
export function beginAndroidOutput(project, outputRoot = defaultAndroidOutputRoot(project)) {
  if (!inside(join(project, 'tmp'), outputRoot)) throw new Error('New Android outputs must be under this project tmp directory.');
  const id = randomUUID(); mkdirSync(outputRoot, { recursive: true });
  const pending = join(outputRoot, `.build-${id}.pending`); mkdirSync(pending);
  return { project, outputRoot, pending, id, work: join(pending, 'work'),
    profile: defaultBuildProfile,
    before: fingerprintMobileSources(project), headers: headerHashes(project),
    cargoLockSha256: sha256(join(project, 'Cargo.lock')), staged: new Set() };
}
export function stageAndroidAbi(run, { abi, archive, measurement, command, nativeStaticLibraries }) {
  const target = androidTargets[abi];
  if (!target || run.staged.has(abi)) throw new Error(`Unexpected or duplicate ABI ${abi}`);
  if (!inside(run.work, archive)) throw new Error('Only freshly produced files from this build work directory can be staged.');
  const after = fingerprintMobileSources(run.project);
  if (after !== run.before) throw new Error('Mobile build sources changed; partial outputs remain unpublished.');
  const declared = headerFunctions(run.project);
  if (!declared.length) throw new Error('No header functions found.');
  const symbols = measurement ?? { kind: 'header-declarations', functions: declared };
  if (!['header-declarations', 'ndk-llvm-nm'].includes(symbols.kind) || !Array.isArray(symbols.functions)) throw new Error('Unknown symbol inventory kind.');
  if (symbols.kind === 'ndk-llvm-nm' && (!symbols.tool || !Array.isArray(symbols.command))) throw new Error('Measured inventory needs actual tool/command provenance.');
  if (declared.some(name => !symbols.functions.includes(name))) throw new Error('Produced archive is missing required header functions.');
  const directory = join(run.pending, abi); mkdirSync(join(directory, 'include'), { recursive: true });
  copyFileSync(archive, join(directory, 'libshellfox_mobile.a'), constants.COPYFILE_EXCL);
  for (const name of headerNames) copyFileSync(join(run.project, 'crates/shellfox-mobile/include', name), join(directory, 'include', name), constants.COPYFILE_EXCL);
  put(join(directory, 'manifest.json'), { format: buildAbiFormat, buildId: run.id, builtFrom: 'live-source-build-output',
    abi, target: target.target, androidApi: 24, sourceFingerprintBefore: run.before, sourceFingerprintAfter: after,
    cargoLockSha256: run.cargoLockSha256, headers: run.headers, profile: run.profile,
    rustArchiveSha256: sha256(join(directory, 'libshellfox_mobile.a')), symbolInventory: symbols,
    producer: 'crates/shellfox-mobile/build-mobile.mjs', producerCommand: command, nativeStaticLibraries,
    evidence: 'build output only; not runtime acceptance or an immutable-source claim' });
  run.staged.add(abi);
}
export function finishAndroidOutput(run) {
  const required = Object.keys(androidTargets);
  if (required.some(abi => !run.staged.has(abi)) || run.staged.size !== required.length) throw new Error('All four required ABIs must finish before publication.');
  const after = fingerprintMobileSources(run.project);
  if (after !== run.before) throw new Error('Mixed source build cannot be published.');
  const outputs = required.map(abi => {
    const file = join(run.pending, abi, 'manifest.json'), manifest = readManifest(file);
    if (manifest.format !== buildAbiFormat || manifest.abi !== abi || manifest.target !== androidTargets[abi].target || manifest.androidApi !== 24
      || manifest.buildId !== run.id || manifest.sourceFingerprintBefore !== run.before || manifest.sourceFingerprintAfter !== after
      || manifest.cargoLockSha256 !== run.cargoLockSha256
      || !Object.entries(run.profile).every(([key, value]) => manifest.profile?.[key] === value)) throw new Error('Mixed ABI build provenance.');
    if (!['header-declarations', 'ndk-llvm-nm'].includes(manifest.symbolInventory?.kind)
      || !Array.isArray(manifest.symbolInventory.functions)
      || headerFunctions(run.project).some(name => !manifest.symbolInventory.functions.includes(name))) throw new Error('Mixed staged symbol declarations.');
    for (const name of headerNames) if (manifest.headers?.[name] !== run.headers[name] || sha256(join(run.pending, abi, 'include', name)) !== run.headers[name]) throw new Error('Mixed staged header inputs.');
    if (sha256(join(run.pending, abi, 'libshellfox_mobile.a')) !== manifest.rustArchiveSha256) throw new Error('Staged archive changed before finalization.');
    return { abi, manifestSha256: sha256(file), archiveSha256: manifest.rustArchiveSha256 };
  });
  put(join(run.pending, 'build-output.json'), { format: buildSetFormat, complete: true, buildId: run.id,
    builtFrom: 'live-source-build-output', requiredAbis: required, sourceFingerprintBefore: run.before,
    sourceFingerprintAfter: after, cargoLockSha256: run.cargoLockSha256, headers: run.headers, profile: run.profile, outputs,
    evidence: 'four-ABI build outputs only; actual JNI/link/runtime acceptance remains separate' });
  const final = join(run.outputRoot, `build-${run.id}`);
  if (existsSync(final)) throw new Error('Refusing to replace a prior build directory.');
  renameSync(run.pending, final);
  const selector = { format: 'shellfox-android-rust-current-v1', directory: `build-${run.id}`, buildId: run.id,
    manifestSha256: sha256(join(final, 'build-output.json')) };
  const current = join(run.outputRoot, 'current.json');
  if (existsSync(current) && readManifest(current).format !== 'shellfox-android-rust-current-v1') throw new Error('Refusing to replace an unknown output selector.');
  const temporary = join(run.outputRoot, `.current-${run.id}.pending`); put(temporary, selector);
  renameSync(temporary, join(run.outputRoot, 'current.json')); // Replaces only the selector, never libraries.
  return final;
}
export function selectedAndroidOutput(project) {
  const outputRoot = defaultAndroidOutputRoot(project);
  const current = join(outputRoot, 'current.json');
  if (!existsSync(current)) throw new Error('No complete Rust Android build output. Run node crates/shellfox-mobile/build-mobile.mjs build-android first, or pass -PshellfoxRustAndroidDir=<dir>.');
  const selector = readManifest(current);
  if (selector.format !== 'shellfox-android-rust-current-v1' || !/^build-[0-9a-f-]{36}$/.test(selector.directory)
    || selector.directory !== `build-${selector.buildId}` || !digest(selector.manifestSha256)) throw new Error('Unknown or invalid default Android output selector.');
  const directory = join(outputRoot, selector.directory), file = join(directory, 'build-output.json');
  if (sha256(file) !== selector.manifestSha256) throw new Error('Default build-output manifest changed.');
  const manifest = readManifest(file);
  if (manifest.format !== buildSetFormat || manifest.complete !== true || manifest.buildId !== selector.buildId) throw new Error('Default Android output is incomplete or unsupported.');
  return directory;
}
