import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { buildHelperWithAnchor, goVersion } from './tailscale.mjs';
import { darwinHelpers } from './terminal-native-helpers.mjs';
import { executableName, fileHash, root, run } from './rust-package-common.mjs';
import { managedInventory, writeManagedAudit } from './rust-package-resource-inventory.mjs';
import { buildWslFilesWorker } from './rust-package-wsl-files.mjs';
import { assertSdk, dotnetEnvironment, helperEnvironment, retainRuntimeNotices, runtimeVersion } from './dotnet-sdk.mjs';

export function writeLaunchers(directory, kind) {
  // Binaries validate --data-dir themselves. No environment-only profile selection.
  const launchers = [['shellfox', 'shellfox-cli']];
  if (kind === 'desktop') launchers.push(['shellfox-desktop', 'shellfox']);
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

export async function stageResources({ directory, work, kind, nugetSnapshot = null, probeTailscale = true, signDarwin }) {
  const resources = path.join(directory, 'resources');
  mkdirSync(resources, { recursive: true });
  const records = [], buildEnv = {};
  // The self-contained .NET runtime ships only with no listed NuGet findings and a supported channel.
  const auditManaged = folder => {
    const inventory = managedInventory(folder, { rid: `win-${process.arch}`, runtimeVersion });
    const report = writeManagedAudit(folder, inventory, nugetSnapshot);
    records.push({ name: 'windows-elevated-terminal-audit', managedAuditStatus: report.status,
      managedRuntimeLifecycleStatus: report.runtimeLifecycle.status });
    if (report.status !== 'no-listed-findings' || report.runtimeLifecycle.status !== 'current-in-listed-channel') {
      const evidence = mkdtempSync(path.join(root, 'tmp', 'rust-package-native-audit-failure-'));
      for (const file of ['managed-inventory.json', 'managed-vulnerability-report.json', 'Shellfox.ElevatedTerminal.deps.json', 'Shellfox.ElevatedTerminal.runtimeconfig.json']) {
        copyFileSync(path.join(folder, file), path.join(evidence, file));
      }
      throw new Error(`Managed runtime security/lifecycle is not cleared. Evidence: ${evidence}`);
    }
  };
  // Keep module-specific notices generated from the Go helper's actual compiled graph.
  const go = process.env.SHELLFOX_GO || 'go';
  const actualGoVersion = run(go, ['version']).trim();
  if (!actualGoVersion.startsWith(`go version go${goVersion} `)) throw new Error(`Packaging requires pinned Go ${goVersion}, found ${actualGoVersion}`);
  const tailscale = path.join(resources, 'tailscale');
  const helper = buildHelperWithAnchor(process.platform, process.arch, { directory: tailscale, probe: probeTailscale });
  if (process.platform === 'darwin') signDarwin?.(helper.executable);
  const helperSha256 = signDarwin ? fileHash(helper.executable) : helper.sha256;
  records.push({ name: 'embedded-tailscale', path: `resources/tailscale/${executableName('shellfox-tailscale')}`, use: 'embedded Tailscale node' });

  const notices = path.join(directory, 'notices', 'ghostty-vt');
  cpSync(path.join(root, 'crates/ghostty-vt-sys/build-support/notices'), notices, { recursive: true });
  if (process.platform === 'win32') {
    mkdirSync(path.join(directory, 'notices', 'conpty'), { recursive: true });
    copyFileSync(path.join(root, 'vendor/conpty/LICENSE'), path.join(directory, 'notices', 'conpty', 'LICENSE'));
    buildEnv.SHELLFOX_WSL_FILES_BINARY = buildWslFilesWorker(work);
    const wslNotices = path.join(directory, 'notices', 'wsl-files');
    mkdirSync(wslNotices, { recursive: true });
    const rustDocs = path.join(run('rustc', ['--print', 'sysroot']).trim(), 'share', 'doc', 'rust');
    // The pinned toolchain supplies musl and the other self-contained runtime objects.
    copyFileSync(path.join(rustDocs, 'COPYRIGHT.html'), path.join(wslNotices, 'COPYRIGHT.html'));
    cpSync(path.join(rustDocs, 'licenses'), path.join(wslNotices, 'licenses'), { recursive: true });
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
      auditManaged(output);
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
      signDarwin?.(file);
      records.push({ name: helper.basename, path: `resources/terminal-native/${helper.basename}`, use: 'macOS process helper' });
    }
  }
  return { records, buildEnv, helperSha256 };
}
