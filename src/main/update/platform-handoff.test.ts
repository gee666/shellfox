import { EventEmitter } from 'node:events';
import path from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
const state = vi.hoisted(() => ({
  files: new Map<string, string>(), calls: [] as { file: string; args: string[] }[],
  packageName: 'shellfox', version: '0.2.6', arch: 'amd64', bundleId: 'local.shellfox', copyFails: false,
}));
vi.mock('node:fs/promises', () => ({
  access: vi.fn(async () => {}), realpath: vi.fn(async (s: string) => s),
  mkdtemp: vi.fn(async (s: string) => s + 'random'), readFile: vi.fn(async () => Buffer.from('archive')),
  rm: vi.fn(async () => {}), writeFile: vi.fn(async (file: string, text: string) => { state.files.set(file.replace(/\\/g, '/'), text); }),
}));
vi.mock('./zip-validation', () => ({ validateUpdateZip: vi.fn() }));
vi.mock('./self-updater', () => ({ hashFile: vi.fn(async () => 'a'.repeat(64)) }));
vi.mock('node:child_process', () => {
  const execFile = vi.fn();
  Object.defineProperty(execFile, Symbol.for('nodejs.util.promisify.custom'), { value: async (file: string, args: string[]) => {
    state.calls.push({ file, args: args.map(arg => arg.replace(/\\/g, '/')) });
    let stdout = '';
    if (file.endsWith('dpkg-query')) stdout = 'shellfox: /usr/lib/shellfox/shellfox';
    if (file.endsWith('dpkg-deb')) stdout = args.at(-1) === 'Package' ? state.packageName : args.at(-1) === 'Version' ? state.version : state.arch;
    if (file.endsWith('PlistBuddy')) stdout = args[1]!.includes('CFBundleIdentifier') ? state.bundleId : state.version;
    if (file.endsWith('ditto') && args.length === 2 && state.copyFails) throw new Error('copy failed');
    return { stdout, stderr: '' };
  } });
  return { execFile, spawn: (file: string, args: string[]) => {
    state.calls.push({ file, args: args.map(arg => arg.replace(/\\/g, '/')) }); const child = new EventEmitter() as EventEmitter & { unref(): void }; child.unref = () => {};
    queueMicrotask(() => child.emit('spawn')); return child;
  } };
});
import { rm } from 'node:fs/promises';
import { validateUpdateZip } from './zip-validation';
import { createUpdatePlatform } from './platform-installer';
vi.mock('./handoff', async importOriginal => ({ ...await importOriginal<typeof import('./handoff')>(), startHandoff: vi.fn(async (file: string, args: string[]) => {
  state.calls.push({ file, args: args.map(arg => arg.replace(/\\/g, '/')) });
  return { committed: false, commit: vi.fn(async () => {}), cancel: vi.fn(async () => {}) };
}) }));
afterEach(() => { state.files.clear(); state.calls = []; state.packageName = 'shellfox'; state.version = '0.2.6'; state.arch = 'amd64'; state.bundleId = 'local.shellfox'; state.copyFails = false; vi.clearAllMocks(); });
describe('Linux handoff', () => {
  it('selects the correct package, checks identity, and waits for quit before hash verification and GUI elevation', async () => {
    const platform = await createUpdatePlatform({ platform: 'linux', arch: 'x64', executable: '/usr/lib/shellfox/shellfox', packaged: true, pid: 456 });
    expect(platform.supported).toBe(true); expect(platform.assetName('0.2.6')).toBe('shellfox_0.2.6_amd64.deb');
    const launch = await platform.prepare('/tmp/work/update.deb', '0.2.6', '/tmp/work');
    expect(state.calls.some(c => c.file === '/bin/sh')).toBe(false);
    const script = state.files.get('/tmp/work/install.sh')!;
    expect(script.indexOf('while kill -0')).toBeLessThan(script.indexOf('/usr/bin/pkexec'));
    expect(script.indexOf('/usr/bin/sha256sum')).toBeLessThan(script.indexOf('/usr/bin/pkexec'));
    expect(script).toContain('"$package"'); expect(script).toContain('/usr/bin/apt-get install -y');
    await launch(); expect(state.calls.at(-1)).toEqual({ file: '/bin/sh', args: ['/tmp/work/install.sh', '456', '/tmp/work', '/tmp/work/update.deb', '/usr/lib/shellfox/shellfox', 'a'.repeat(64), '/tmp/work/handoff-random'] });
  });
  it.each(['name', 'version', 'arch'])('rejects incorrect Debian package %s', async kind => {
    if (kind === 'name') state.packageName = 'attacker'; if (kind === 'version') state.version = '0.2.5'; if (kind === 'arch') state.arch = 'arm64';
    const platform = await createUpdatePlatform({ platform: 'linux', arch: 'x64', executable: '/usr/lib/shellfox/shellfox', packaged: true });
    await expect(platform.prepare('/tmp/package', '0.2.6', '/tmp/work')).rejects.toThrow('Unexpected Debian package');
    expect(state.files.size).toBe(0);
  });
});
describe('macOS handoff', () => {
  const options = { platform: 'darwin', arch: 'arm64', executable: '/Applications/Shellfox.app/Contents/MacOS/Shellfox', packaged: true, pid: 789 };
  it('validates before extraction, checks the signed bundle, stages beside the app and supports rollback', async () => {
    const platform = await createUpdatePlatform(options);
    expect(platform.assetName('0.2.6')).toBe('Shellfox-darwin-arm64-0.2.6.zip');
    const launch = await platform.prepare('/tmp/work/update.zip', '0.2.6', '/tmp/work');
    expect(validateUpdateZip).toHaveBeenCalledTimes(1);
    expect(state.calls).toContainEqual({ file: '/usr/bin/codesign', args: ['--verify', '--deep', '--strict', '/tmp/work/extracted/Shellfox.app'] });
    await launch();
    expect(state.calls.at(-1)).toEqual({ file: '/bin/sh', args: ['/tmp/work/install.sh', '789', '/tmp/work', '/Applications/Shellfox.app', '/Applications/.Shellfox-update-random', '/tmp/work/handoff-random'] });
    const script = state.files.get('/tmp/work/install.sh')!;
    expect(script.indexOf('while kill -0')).toBeLessThan(script.indexOf('mv --'));
    expect(script).toContain('mv -- "$old" "$bundle"'); expect(script).toContain('/usr/bin/open -n "$bundle"');
    await platform.cleanup?.(); expect(rm).toHaveBeenCalledWith(path.normalize('/Applications/.Shellfox-update-random'), { recursive: true, force: true });
  });
  it('cleans sibling staging after a copy failure', async () => {
    state.copyFails = true;
    const platform = await createUpdatePlatform(options);
    await expect(platform.prepare('/tmp/work/update.zip', '0.2.6', '/tmp/work')).rejects.toThrow();
    expect(rm).toHaveBeenCalledWith(path.normalize('/Applications/.Shellfox-update-random'), { recursive: true, force: true });
    expect(state.calls.some(c => c.file === '/bin/sh')).toBe(false);
  });
  it('rejects a bundle with a different version before staging or quitting', async () => {
    const platform = await createUpdatePlatform(options); state.version = '0.2.5';
    await expect(platform.prepare('/tmp/work/update.zip', '0.2.6', '/tmp/work')).rejects.toThrow('Unexpected app bundle');
    expect(state.files.size).toBe(0);
  });
});
