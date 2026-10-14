import { afterEach, expect, it, vi } from 'vitest';
import path from 'node:path';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { CustomCommandStore } from './shellfox-cli-commands';
import { CUSTOM_COMMAND_HELP, dispatchCustomCli } from './shellfox-cli-dispatch';
import { resolveScriptLocation, scriptInvocation, customScriptContext } from './shellfox-cli-scripts';

const roots: string[] = [];
afterEach(async () => { vi.restoreAllMocks(); await Promise.all(roots.splice(0).map(root => rm(root, { recursive: true, force: true }))); });
async function fixture() {
  await mkdir(path.resolve('tmp'), { recursive: true });
  const root = await mkdtemp(path.resolve('tmp/custom-cli-')); roots.push(root);
  const file = path.join(root, 'script with spaces.sh'); await writeFile(file, 'printf "%s\\n" "$@"\n');
  const store = new CustomCommandStore(path.join(root, 'data'));
  const output = { write: vi.fn() }, error = { write: vi.fn() };
  return { root, file, store, output, error };
}
it('keeps existing version-1 absolute references and reads current contents without copying scripts', async () => {
  const { root, file, store, output, error } = await fixture();
  const location = await resolveScriptLocation(path.basename(file), { platform: process.platform, cwd: root, env: {} });
  await store.register('Build', location);
  await writeFile(file, 'updated contents');
  expect(await new CustomCommandStore(store.userData).read()).toEqual([{ name: 'Build', path: file }]);
  expect(await readFile(store.file, 'utf8')).not.toContain('updated contents');
  const execute = vi.fn(async () => 0);
  expect(await dispatchCustomCli(['cmd', 'Build'], store.userData, { execute, output, error })).toBe(0);
  expect(execute).toHaveBeenCalledWith({ name: 'Build', path: file }, [], expect.anything());
});
it('registers, lists paths and renames case-insensitively under cmd only', async () => {
  const { file, store, output, error } = await fixture();
  const options = { output, error };
  expect(await dispatchCustomCli(['cmd', '--register', 'Build', file], store.userData, options)).toBe(0);
  for (const flag of ['--list', '--ls']) {
    output.write.mockClear();
    expect(await dispatchCustomCli(['cmd', flag], store.userData, options)).toBe(0);
    expect(output.write).toHaveBeenCalledWith('Build\t' + JSON.stringify(file) + '\n');
  }
  expect(await dispatchCustomCli(['cmd', '--rename', 'BUILD', 'deploy'], store.userData, options)).toBe(0);
  expect(await store.read()).toEqual([{ name: 'deploy', path: file }]);
  expect(error.write).not.toHaveBeenCalled();
});
it.each(['--remove', '--rm', '--unregister'])('supports removal through %s without deleting the script', async flag => {
  const { file, store, output, error } = await fixture(); await store.register('Build', { path: file });
  expect(await dispatchCustomCli(['cmd', flag, 'BUILD'], store.userData, { output, error })).toBe(0);
  expect(await store.read()).toEqual([]); expect(await readFile(file, 'utf8')).toContain('printf');
  expect(error.write).not.toHaveBeenCalled();
});
it('rejects custom duplicates, unsafe names and rename overwrites, but allows builtin names', async () => {
  const { file, store } = await fixture();
  await store.register('hello', { path: file }); await store.register('other', { path: file });
  await expect(store.register('HELLO', { path: file })).rejects.toThrow('already registered');
  for (const name of ['bad name', 'bad;echo', '../bad', '-h', '', 'a'.repeat(65), '__proto__']) {
    await expect(store.register(name, { path: file })).rejects.toThrow();
  }
  await expect(store.rename('hello', 'OTHER')).rejects.toThrow('already registered');
  await store.rename('hello', 'ls');
  await expect(store.rename('missing', 'fine')).rejects.toThrow('No custom');
  await expect(store.unregister('missing')).rejects.toThrow('No custom');
  expect((await store.read()).map(command => command.name)).toEqual(['ls', 'other']);
});
it('allows all top-level builtins and former management names without warnings', async () => {
  const { file, store, output, error } = await fixture(); const execute = vi.fn(async () => 17);
  const names = ['start', 'add', 'new', 'create', 'cmd', 'ssh', 'update', 'help', 'version', 'ls', 'list', 'sessions',
    'delete', 'remove', 'rm', 'archive', 'settle', 'keep-alive', 'keepalive', 'awake', 'stats', 'status', 'usage',
    'processes', 'proc', 'ps', 'tab', 'register', 'unregister', 'rename', 'commands', 'future'];
  for (const name of names) {
    expect(await dispatchCustomCli(['cmd', '--register', name, file], store.userData, { execute, output, error })).toBe(0);
    expect(await dispatchCustomCli(['cmd', '--run', name.toUpperCase(), '--help'], store.userData, { execute, output, error })).toBe(17);
  }
  expect(execute).toHaveBeenCalledTimes(names.length);
  expect(error.write).not.toHaveBeenCalled();
});
it('serializes concurrent writers and does not lose successful registrations', async () => {
  const { file, store } = await fixture();
  await Promise.all(Array.from({ length: 8 }, (_, i) => new CustomCommandStore(store.userData).register('cmd' + i, { path: file })));
  expect(await store.read()).toHaveLength(8);
  const results = await Promise.allSettled([store.register('same', { path: file }), store.register('SAME', { path: file })]);
  expect(results.filter(result => result.status === 'fulfilled')).toHaveLength(1);
});
it.each([false, true])('passes every argument after the name untouched, explicit --run: %s', async explicit => {
  const { file, store, output, error } = await fixture(); await store.register('echoargs', { path: file });
  const execute = vi.fn(async () => 27);
  const args = ['', 'two words', '"quoted"', "it's", '--help', '-h', 'help', '--register', '--remove', '--rm',
    '--unregister', '--rename', '--list', '--ls', '--run', '--name=x', '$(touch injected)', '; echo bad', '& whoami', '%PATH%', '!x!', '\n'];
  expect(await dispatchCustomCli(['cmd', ...(explicit ? ['--run'] : []), 'ECHOARGS', ...args], store.userData, { execute, output, error })).toBe(27);
  expect(execute).toHaveBeenCalledWith({ name: 'echoargs', path: file }, args, expect.anything());
  expect(await store.read()).toEqual([{ name: 'echoargs', path: file }]); expect(error.write).not.toHaveBeenCalled();
});
it('does not intercept top-level names or read the registry outside cmd', async () => {
  const { store, output, error } = await fixture(); const execute = vi.fn(async () => 0);
  const read = vi.spyOn(CustomCommandStore.prototype, 'read').mockRejectedValue(new Error('must not read'));
  for (const argv of [['register', 'name', '/file.sh'], ['rename', 'old', 'new'], ['unregister', 'name'],
    ['commands'], ['list', '--commands'], ['ls', '--commands'], ['list'], ['ls', '--running'], ['start'],
    ['ssh'], ['stats'], ['unknown'], ['help', 'register'], ['--shellfox-check-custom', 'start']]) {
    expect(await dispatchCustomCli(argv, store.userData, { execute, output, error })).toBeNull();
  }
  expect(read).not.toHaveBeenCalled(); expect(execute).not.toHaveBeenCalled();
  expect(output.write).not.toHaveBeenCalled(); expect(error.write).not.toHaveBeenCalled();
});
it('reports missing scripts, malformed operations and a corrupt registry only inside cmd', async () => {
  const { store, output, error } = await fixture(); const execute = vi.fn(async () => 0);
  expect(await dispatchCustomCli(['cmd', 'missing'], store.userData, { execute, output, error })).toBe(1);
  for (const args of [['--register'], ['--register', 'name'], ['--rename', 'old'], ['--rm'], ['--run'],
    ['--ls', 'extra'], ['--help', 'extra'], ['--unknown']]) {
    expect(await dispatchCustomCli(['cmd', ...args], store.userData, { execute, output, error })).toBe(2);
  }
  expect(execute).not.toHaveBeenCalled();
  await mkdir(store.userData); await writeFile(store.file, '{broken');
  error.write.mockClear();
  expect(await dispatchCustomCli(['list'], store.userData, { output, error })).toBeNull();
  expect(error.write).not.toHaveBeenCalled();
  expect(await dispatchCustomCli(['cmd', '--ls'], store.userData, { output, error })).toBe(1);
  await expect(store.register('newcmd', { path: '/script.sh' })).rejects.toThrow();
  expect(await readFile(store.file, 'utf8')).toBe('{broken');
});
it('shows namespace help only before a name, and lets a script be named help', async () => {
  const { file, store, output, error } = await fixture(); const execute = vi.fn(async () => 0);
  for (const args of [[], ['--help'], ['-h']]) {
    expect(await dispatchCustomCli(['cmd', ...args], store.userData, { output, error })).toBe(0);
    expect(output.write).toHaveBeenLastCalledWith(CUSTOM_COMMAND_HELP);
  }
  await store.register('help', { path: file });
  expect(await dispatchCustomCli(['cmd', 'help', '--help'], store.userData, { execute, output, error })).toBe(0);
  expect(execute).toHaveBeenCalledWith({ name: 'help', path: file }, ['--help'], expect.anything());
});
it('rejects directories, absent paths, path controls and relative on-disk entries', async () => {
  const { root, file, store } = await fixture();
  await expect(resolveScriptLocation(root)).rejects.toThrow('file');
  await expect(resolveScriptLocation(file + '\0')).rejects.toThrow('NUL');
  await expect(resolveScriptLocation(file + '.missing')).rejects.toThrow();
  await expect(store.register('bad', { path: 'relative.sh' })).rejects.toThrow('registry');
});
it('builds shell-free POSIX and WSL invocations, preserving argv and caller directories', async () => {
  const args = ['two words', '$(echo bad)', '"', '', '--help'];
  const posix = await scriptInvocation({ path: "/path/it's a file.sh" }, args, { platform: 'linux', cwd: '/work', env: { ELECTRON_RUN_AS_NODE: '1' } });
  expect(posix).toEqual({ executable: 'bash', args: ['--', "/path/it's a file.sh", ...args], cwd: '/work', env: {} });
  const wsl = await scriptInvocation({ path: '/home/me/script space.sh', distro: 'Ubuntu' }, args,
    { platform: 'win32', cwd: 'C:\\work', env: { SHELLFOX_CLI_DISTRO: 'Ubuntu', SHELLFOX_CLI_WSL_CWD: '/home/me/work' } });
  expect(wsl.args).toEqual(['--distribution', 'Ubuntu', '--cd', '/home/me/work', '--exec', '/bin/bash', '--', '/home/me/script space.sh', ...args]);
  expect(wsl.executable).toBe('C:\\Windows\\System32\\wsl.exe');
  expect(wsl.args).not.toContain('-c');
});
it('rejects batch execution and encodes PowerShell values as data, not command text', async () => {
  const context = { platform: 'win32' as const, cwd: 'C:\\work', env: {} };
  await expect(scriptInvocation({ path: 'C:\\unsafe.cmd' }, [], context)).rejects.toThrow('Batch');
  const args = ['$(Set-Content injected bad)', '"quoted"', '', '%PATH%', '-flag', '\n'];
  const invocation = await scriptInvocation({ path: "C:\\space dir\\it's.ps1" }, args, context);
  const code = Buffer.from(invocation.args.at(-1)!, 'base64').toString('utf16le');
  const data = JSON.parse(Buffer.from(/FromBase64String\('([^']+)'\)/.exec(code)![1], 'base64').toString());
  expect(data).toEqual({ path: "C:\\space dir\\it's.ps1", args });
  expect(code).not.toContain('Set-Content injected'); expect(code).not.toContain('Invoke-Expression');
  expect(code).toContain('@scriptArgs');
});
it('adds WSL context through WSLENV and records Git Bash as an absolute interpreter', async () => {
  const { posixHelper } = await import('./cli-helper');
  const shim = customScriptContext(posixHelper({ executable: 'C:\\app.exe' }, true));
  expect(shim).toContain('export SHELLFOX_CLI_WSL_CWD="$PWD"');
  expect(shim).toContain('shellfox_wsl_env SHELLFOX_CLI_DISTRO');
  expect(shim).toContain('shellfox_wsl_env SHELLFOX_CLI_WSL_CWD');
  expect(shim).toContain('command -v bash'); expect(shim).toContain('SHELLFOX_CLI_GIT_CWD');
});
