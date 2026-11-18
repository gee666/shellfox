// Opt-in Native002 producer composition, not accepted release artifacts.
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { existsSync, lstatSync, readFileSync, readdirSync, readlinkSync, writeFileSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import { archives, support, version, zigVersion } from './archives.mjs';

export const experimentalStatus = 'experimental-native002-not-final-acceptance';
export const patchPins = Object.freeze([
  { order: 1, path: 'patches/ansi-export/checkpoints/native-002/0001-ansi-export.patch', sha256: '68d3e2d557594ae91bf4655c6a4fc9bde5a89cbe3958053cb61b0e09cd00183d' },
  { order: 2, path: 'patches/terminal-profile/checkpoints/native-002/0002-terminal-profile.patch', sha256: '8cc4f2403f4fe69faf1b77916d2db8ac3f713824dbfe273ba714c0a3fdf02d22' },
  { order: 3, path: 'patches/snapshot-budget/checkpoints/native-002/0003-snapshot-budget.patch', sha256: '28fd8c68edebdd8a014c40123261a2494ea3bde47089b3c2e4dd3cf474012a00' },
]);
export const headerPins = Object.freeze({
  'ghostty/vt/shellfox_extension.h': 'a640920dbbecdb8759205250519d56978126c2dbf2524b585fd8b7eac346b763',
  'ghostty/vt/shellfox_ansi.h': '4b8da0ec379a0dcfd5b4972b66981b4fed55019d70e304b17b5a4adeb94711dd',
  'ghostty/vt/shellfox_terminal_profile.h': '06fb609354abaffcf5e95864c812d8658e6e78e5bf4cbf78376e03dc0dd36967',
  'ghostty/vt/shellfox_snapshot_budget.h': 'fa35639f1759a7804f6953cc68f467b3e664d33352bd8b6a5a756deb71a8a3a3',
});
export const unicodePin = '2167c46bf05f2ecbb35a247f11216d44664b52c47c63bee86947823ddfa1fc90';
export const sha256 = value => createHash('sha256').update(value).digest('hex');

export function verifyExperimentalInputs(read = readFileSync) {
  return patchPins.map(pin => {
    const bytes = read(join(support, pin.path));
    if (sha256(bytes) !== pin.sha256) throw new Error(`Experimental patch SHA-256 mismatch: ${pin.path}`);
    const checkpointPath = join(support, pin.path, '..', 'checkpoint.json');
    const checkpoint = JSON.parse(read(checkpointPath).toString('utf8'));
    if (checkpoint.patchSha256 !== pin.sha256 || checkpoint.patchOrder !== pin.order) {
      throw new Error(`Experimental checkpoint mismatch: ${pin.path}`);
    }
    const requires = checkpoint.requires ? [].concat(checkpoint.requires) : [];
    for (const previous of patchPins.slice(0, pin.order - 1)) {
      if (!requires.some(value => value.patchOrder === previous.order && value.patchSha256 === previous.sha256)) {
        throw new Error(`Experimental prerequisite mismatch: ${pin.path}`);
      }
    }
    return { ...pin, bytes, checkpointSha256: sha256(read(checkpointPath)) };
  });
}

export function experimentalRecipe(rustTarget, zigTarget) {
  for (const value of [rustTarget, zigTarget]) {
    if (!/^[a-zA-Z0-9_.+-]+$/.test(value)) throw new Error('Invalid experimental target spelling');
  }
  const lines = [
    'shellfox-native002 1',
    `source ${archives['libghostty-vt-1.0.1.tar.gz']}`,
    `deps ${archives['libghostty-vt-1.0.1-deps.tar.gz']}`,
    ...patchPins.map(pin => `patch${pin.order} ${pin.sha256}`),
    `zig ${zigVersion}`, 'optimize ReleaseSafe', 'cpu baseline',
    `rust-target ${rustTarget}`, `zig-target ${zigTarget}`,
    'features upstream-default', 'app-version 1.4.1-dev', `lib-version ${version}`,
    ...Object.entries(headerPins).map(([name, digest]) => `header-${name.split('/').at(-1)} ${digest}`),
    `unicode ${unicodePin}`, 'terminal-profile native-only',
  ];
  const text = `${lines.join('\n')}\n`;
  return { text, engineProfile: sha256(text), rustTarget, zigTarget };
}

export function experimentalBuildArgs(recipe) {
  return ['-Doptimize=ReleaseSafe', '-Dversion-string=1.4.1-dev', `-Dlib-version-string=${version}`,
    `-Dshellfox-engine-profile=${recipe.engineProfile}`, '-Demit-lib-vt', '-j1'];
}

export function applyExperimental(source, patch = 'patch') {
  const verified = verifyExperimentalInputs();
  if (readFileSync(join(source, 'LIB_VERSION'), 'utf8').trim() !== version) throw new Error('Not pinned vt1.0.1 source');
  if (existsSync(join(source, 'include/ghostty/vt/shellfox_extension.h'))) throw new Error('Experimental source is already patched');
  for (const pin of verified) {
    for (const dry of [true, false]) {
      const args = [...(dry ? ['--dry-run'] : []), '--batch', '--forward', '--fuzz=0',
        '--no-backup-if-mismatch', '--reject-file=-', '-p1'];
      // Feed the verified immutable bytes, not a producer path that could change.
      const result = spawnSync(patch, args, { cwd: source, input: pin.bytes, encoding: 'utf8',
        timeout: 120_000, maxBuffer: 4 * 1024 * 1024 });
      if (result.error || result.status !== 0) {
        throw new Error(`Experimental patch${pin.order} ${dry ? 'preflight' : 'apply'} failed: ${result.error ?? result.stderr + result.stdout}. Supply --patch with a GNU patch executable`);
      }
    }
  }
  verifyExperimentalHeaders(source);
  if (sha256(readFileSync(join(source, 'src/terminal/shellfox_unicode_v6.zig'))) !== unicodePin) {
    throw new Error('Experimental shared Unicode module differs from its pin');
  }
  return verified.map(({ bytes, ...record }) => record);
}

export function verifyExperimentalHeaders(prefix) {
  for (const [name, expected] of Object.entries(headerPins)) {
    if (sha256(readFileSync(join(prefix, 'include', name))) !== expected) throw new Error(`Experimental header mismatch: ${name}`);
  }
}

export function sourceTreeDigest(source) {
  const entries = [];
  function visit(directory) {
    for (const name of readdirSync(directory).sort()) {
      const path = join(directory, name), info = lstatSync(path);
      const key = relative(source, path).replaceAll('\\', '/');
      if (info.isDirectory()) visit(path);
      else if (info.isSymbolicLink()) entries.push(`${key}\0link:${readlinkSync(path)}\n`);
      else if (info.isFile()) entries.push(`${key}\0${sha256(readFileSync(path))}\n`);
      else throw new Error(`Unsupported source-tree entry: ${key}`);
    }
  }
  visit(resolve(source));
  return sha256(entries.sort().join(''));
}

export function writeExperimentalManifest(prefix, recipe, patches, sourceDigest, windows) {
  verifyExperimentalHeaders(prefix);
  const files = [
    ...Object.keys(headerPins).map(name => `include/${name}`),
    'lib/pkgconfig/libghostty-vt.pc', 'lib/pkgconfig/libghostty-vt-static.pc',
    ...(windows ? ['lib/ghostty-vt.lib', 'bin/ghostty-vt.dll'] : ['lib/libghostty-vt.a']),
  ];
  const artifacts = files.map(path => ({ path, sha256: sha256(readFileSync(join(prefix, path))) }));
  writeFileSync(join(prefix, 'shellfox-native002.recipe'), recipe.text, { flag: 'wx' });
  writeFileSync(join(prefix, 'shellfox-native002.files'),
    `${artifacts.map(file => `${file.sha256}  ${file.path}`).join('\n')}\n`, { flag: 'wx' });
  writeFileSync(join(prefix, 'shellfox-native002.json'), `${JSON.stringify({
    schema: 1, status: experimentalStatus, engineProfile: recipe.engineProfile,
    profileIsAuthenticity: false, acceptedReleaseArtifact: false,
    sourceDigestBeforeAndAfterBuild: sourceDigest, patches,
    abi: { extension: 1, ansi: 1, terminalProfile: 1, snapshotBudget: 1, snapshotVersion: 1,
      supportedTerminalProfiles: [0], legacyUnsupported: true },
    recipe: recipe.text, artifacts,
  }, null, 2)}\n`, { flag: 'wx' });
}
