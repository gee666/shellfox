import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { buildHelper, goVersion, helperTarget, probeHelper } from './tailscale.mjs';
import { darwinHelpers } from './terminal-native-helpers.mjs';
import { executableName, root, run } from './rust-package-common.mjs';
import { generateResourceManifest } from './rust-package-resource-manifest.mjs';
import { assertSdk, dotnetEnvironment, helperEnvironment, retainRuntimeNotices, runtimeVersion } from './dotnet-sdk.mjs';

export function writeLaunchers(directory, kind) {
  // Binaries validate --data-dir themselves. No environment-only profile selection.
  const launchers = [['shellfox', 'shellfox-cli']];
  if (kind === 'desktop') launchers.push(['shellfox-desktop', 'shellfox-gpui-experiment']);
  for (const [name, bin] of launchers) {
    if (process.platform === 'win32') {
      writeFileSync(path.join(directory, name + '.cmd'), `@echo off\r\n"%~dp0bin\\${bin}.exe" %*\r\nexit /b %errorlevel%\r\n`);
      writeFileSync(path.join(directory, name + '.ps1'), `$ErrorActionPreference = 'Stop'\r\n& (Join-Path $PSScriptRoot 'bin/${bin}.exe') @args\r\nexit $LASTEXITCODE\r\n`);
    } else {
      const file = path.join(directory, name);
      writeFileSync(file, `#!/bin/sh\nset -eu\nroot=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)\nexec "$root/bin/${bin}" "$@"\n`);
      chmodSync(file, 0o755);
    }
  }
}

export async function stageResources({ directory, work, kind, version, nugetSnapshot = null }) {
  const resources = path.join(directory, 'resources');
  mkdirSync(resources, { recursive: true });
  const records = [], anchorEnv = {
    SHELLFOX_WINDOWS_HELPER_MANIFEST_SHA256: '', SHELLFOX_GO_HELPER_MANIFEST_SHA256: '',
    SHELLFOX_DARWIN_HELPERS_MANIFEST_SHA256: '', SHELLFOX_WSL_SCRIPTS_MANIFEST_SHA256: '',
  };
  const rid = `${{ win32: 'win', darwin: 'osx', linux: 'linux' }[process.platform]}-${process.arch}`;
  const makeBundle = (bundleKind, folder, runtimeVersion, name, variable, probe = false) => {
    const record = generateResourceManifest({ directory: folder, kind: bundleKind, rid, version, runtimeVersion,
      provenance: path.join(directory, 'trust-anchors', `${name}.json`), nugetSnapshot, probe });
    anchorEnv[variable] = record.expectedManifestDigest;
    records.push({ name: `${name}-bundle`, identity: record.identity, expectedManifestDigest: record.expectedManifestDigest,
      trustAnchor: `trust-anchors/${name}.json`, managedAuditStatus: record.managedAuditStatus,
      managedRuntimeLifecycleStatus: record.managedRuntimeLifecycleStatus });
    if (bundleKind === 'managed' && (record.managedAuditStatus !== 'no-listed-findings' || record.managedRuntimeLifecycleStatus !== 'current-in-listed-channel')) {
      const evidence = mkdtempSync(path.join(root, 'tmp', 'rust-package-native-audit-failure-'));
      for (const file of ['managed-inventory.json', 'managed-vulnerability-report.json', 'bundle-manifest.json', 'Shellfox.ElevatedTerminal.deps.json', 'Shellfox.ElevatedTerminal.runtimeconfig.json']) {
        copyFileSync(path.join(folder, file), path.join(evidence, file));
      }
      throw new Error(`Managed native security/lifecycle is not cleared. No artifact or compiled anchor accepted. Evidence: ${evidence}`);
    }
  };
  // Keep module-specific notices generated from the Go helper's actual compiled graph.
  const go = process.env.SHELLFOX_GO || 'go';
  const actualGoVersion = run(go, ['version']).trim();
  if (!actualGoVersion.startsWith(`go version go${goVersion} `)) throw new Error(`Packaging requires pinned Go ${goVersion}, found ${actualGoVersion}`);
  buildHelper();
  cpSync(helperTarget().directory, path.join(resources, 'tailscale'), { recursive: true });
  records.push({ name: 'embedded-tailscale', path: `resources/tailscale/${executableName('shellfox-tailscale')}`, use: 'retained helper; Rust adapter acceptance required' });
  probeHelper(path.join(resources, 'tailscale', executableName('shellfox-tailscale')));
  makeBundle('go', path.join(resources, 'tailscale'), goVersion, 'go-helper', 'SHELLFOX_GO_HELPER_MANIFEST_SHA256', true);

  const notices = path.join(directory, 'notices', 'ghostty-vt');
  cpSync(path.join(root, 'crates/ghostty-vt-sys/build-support/notices'), notices, { recursive: true });
  if (process.platform === 'win32') {
    if (kind === 'desktop') {
      const output = path.join(resources, 'elevated-terminal');
      const dotnet = assertSdk();
      run(dotnet, ['publish', 'native/Shellfox.ElevatedTerminal/Shellfox.ElevatedTerminal.csproj',
        '-c', 'Release', '-r', `win-${process.arch}`, '--self-contained', 'true', '-o', output,
        `-p:BaseIntermediateOutputPath=${path.join(work, 'dotnet-obj')}${path.sep}`,
        `-p:BaseOutputPath=${path.join(work, 'dotnet-bin')}${path.sep}`], { env: dotnetEnvironment(dotnet) });
      for (const name of ['Shellfox.ElevatedTerminal.exe', 'Shellfox.ElevatedTerminal.dll',
        'Shellfox.ElevatedTerminal.deps.json', 'Shellfox.ElevatedTerminal.runtimeconfig.json', 'hostfxr.dll', 'hostpolicy.dll', 'coreclr.dll']) {
        if (!existsSync(path.join(output, name))) throw new Error(`Missing self-contained Windows helper asset: ${name}`);
      }
      const result = run(path.join(output, 'Shellfox.ElevatedTerminal.exe'), ['probe'], { timeout: 15_000, env: helperEnvironment() }).trim();
      if (result !== 'shellfox-elevated-conpty-v1') throw new Error('Windows elevated helper probe failed');
      // Standalone and packaged helper builds retain the same pinned notices.
      await retainRuntimeNotices(output);
      records.push({ name: 'windows-elevated-terminal', path: 'resources/elevated-terminal/Shellfox.ElevatedTerminal.exe', use: 'retained self-contained helper; no UAC launched during packaging' });
      makeBundle('managed', output, runtimeVersion, 'windows-helper', 'SHELLFOX_WINDOWS_HELPER_MANIFEST_SHA256', true);
    }
  } else if (process.platform === 'darwin') {
    const output = path.join(resources, 'terminal-native');
    mkdirSync(output, { recursive: true });
    for (const helper of darwinHelpers) {
      const file = path.join(output, helper.basename);
      run('xcrun', ['--sdk', 'macosx', 'clang', '-std=c11', '-O2', '-Wall', '-Wextra', '-Werror',
        '-arch', process.arch === 'x64' ? 'x86_64' : 'arm64', '-mmacosx-version-min=13.0',
        path.join(root, 'src/main/terminal/native', helper.sourceFile), '-lproc', '-o', file]);
      chmodSync(file, 0o755);
      records.push({ name: helper.basename, path: `resources/terminal-native/${helper.basename}`, use: 'retained native helper; Darwin runtime acceptance required' });
    }
    makeBundle('darwin', output, undefined, 'darwin-helpers', 'SHELLFOX_DARWIN_HELPERS_MANIFEST_SHA256');
  }
  return { records, anchorEnv };
}
