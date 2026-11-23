import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { test } from 'node:test';
import { channel, dotnetCommand, helperEnvironment, projectRoot, releaseFile, runtimeVersion, sdkVersion, windowsSection } from './dotnet-sdk.mjs';

const metadata = () => ({
  'channel-version': channel, 'support-phase': 'active', 'eol-date': '2028-11-14',
  'latest-sdk': sdkVersion, 'latest-runtime': runtimeVersion,
  releases: [{ runtime: { version: runtimeVersion }, sdk: { version: sdkVersion, files: [{
    rid: 'win-x64', name: 'dotnet-sdk-win-x64.zip', hash: 'a'.repeat(128),
    url: `https://builds.dotnet.microsoft.com/dotnet/Sdk/${sdkVersion}/dotnet-sdk-${sdkVersion}-win-x64.zip`,
  }] } }],
});
test('SDK archive requires exact official Microsoft identity, SHA512 and supported current pins', () => {
  assert.equal(releaseFile(metadata(), 'x64', new Date('2026-11-16')).hash, 'a'.repeat(128));
  for (const changed of [
    { 'support-phase': 'eol' }, { 'support-phase': 'preview' }, { 'eol-date': '2026-11-10' }, { 'eol-date': 'not-a-date' },
    { 'latest-sdk': '10.0.1' }, { 'latest-runtime': '10.0.1' }, { 'channel-version': '8.0' },
  ]) assert.throws(() => releaseFile({ ...metadata(), ...changed }, 'x64', new Date('2026-11-16')));
  for (const changed of [{ hash: 'bad' }, { url: 'https://untrusted.invalid/sdk.zip' }, { rid: 'win-arm64' }]) {
    const value = metadata(); Object.assign(value.releases[0].sdk.files[0], changed);
    assert.throws(() => releaseFile(value, 'x64', new Date('2026-11-16')));
  }
});
test('managed probes remove every CLR hook alias before pre-entry code can run', () => {
  const source = { Path: 'kept', DOTNET_STARTUP_HOOKS: 'blocked', cOmPlUs_Version: 'blocked', CORECLR_ENABLE_PROFILING: '1', COR_PROFILER_PATH: 'blocked', DevPath: 'blocked' };
  const result = helperEnvironment(source);
  assert.equal(Object.keys(result).length, 5);
  assert.ok(Object.values(result).every(value => value === undefined));
  assert.equal(Object.hasOwn(result, 'Path'), false);
  assert.equal(source.COR_PROFILER_PATH, 'blocked');
});

test('absolute SDK override is active-only and never searches PATH or executes a fixture', () => {
  const temporary = path.join(projectRoot, 'tmp'); mkdirSync(temporary, { recursive: true });
  const root = mkdtempSync(path.join(temporary, 'dotnet-resolver-test-'));
  try {
    const file = path.join(root, 'dotnet.exe'); writeFileSync(file, 'owned inert fixture');
    assert.equal(dotnetCommand(file), file);
    assert.throws(() => dotnetCommand('dotnet'));
    assert.throws(() => dotnetCommand(path.join(root, 'missing', 'dotnet.exe')));
    assert.throws(() => dotnetCommand(path.join(root, 'powershell.exe')));
  } finally { rmSync(root, { recursive: true, force: true }); }
});
test('support evidence must explicitly retain Windows1809 and Arm64/x64', () => {
  assert.throws(() => windowsSection('## Windows\nWindows11 only\n## Other\n'));
  const table = '## Windows\nWindows10 1809 | Arm64, x64\n\n[0]: example\n';
  assert.equal(windowsSection(table), 'Windows10 1809 | Arm64, x64');
});
test('active project and runtime pins agree and pre-entry managed startup hooks are disabled', () => {
  const project = readFileSync(path.join(projectRoot, 'native/Shellfox.ElevatedTerminal/Shellfox.ElevatedTerminal.csproj'), 'utf8');
  assert.ok(project.includes('<TargetFramework>net10.0-windows</TargetFramework>'));
  assert.ok(project.includes(`<RuntimeFrameworkVersion>${runtimeVersion}</RuntimeFrameworkVersion>`));
  assert.match(project, /RuntimeHostConfigurationOption Include="System.StartupHookProvider.IsSupported" Value="false"/);
  const program = readFileSync(path.join(projectRoot, 'native/Shellfox.ElevatedTerminal/Program.cs'), 'utf8');
  assert.ok(program.includes('OperatingSystem.IsWindowsVersionAtLeast(10, 0, 17763)'));
});
test('reviewed helper has no payload-selected managed/native library loader calls', () => {
  const directory = path.join(projectRoot, 'native/Shellfox.ElevatedTerminal');
  let imports = 0;
  for (const file of readdirSync(directory).filter(file => file.endsWith('.cs'))) {
    const source = readFileSync(path.join(directory, file), 'utf8');
    imports += (source.match(/\[DllImport\(/g) ?? []).length;
    assert.doesNotMatch(source, /\b(?:NativeLibrary|AssemblyLoadContext|AssemblyDependencyResolver|Assembly\.(?:Load\w*|UnsafeLoadFrom)|Activator|Type\.GetType|Reflection\.Emit|LoadLibrary\w*|GetProcAddress|SetDllDirectory\w*|AddDllDirectory|SetDllImportResolver|GetDelegateForFunctionPointer)\b/, file);
  }
  assert.equal(imports, 39, 'New import owners require review and compiled reflection coverage.');
  // Regression guard; new loader-related imports still need human review.
});
