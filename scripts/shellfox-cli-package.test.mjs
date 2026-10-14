import assert from 'node:assert/strict';
import { test } from 'node:test';
import path from 'node:path';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { createRequire } from 'node:module';
import { build } from 'esbuild';
import { buildShellfoxCli } from './shellfox-cli-build.mjs';
const exec = promisify(execFile), require = createRequire(import.meta.url);

test('production CLI bundle and staged runtime execute custom commands from the packaged resource layout', { timeout: 420000 }, async () => {
  await mkdir('tmp', { recursive: true });
  const root = await mkdtemp(path.resolve('tmp/cli package-'));
  try {
    const buildScript = await readFile('scripts/build.mjs', 'utf8');
    assert.ok(buildScript.includes("import { buildShellfoxCli } from './shellfox-cli-build.mjs'"));
    assert.ok(buildScript.includes("await buildShellfoxCli(output + '/cli')"));
    const resources = path.join(root, 'resources', 'cli');
    await buildShellfoxCli(resources);
    const version = JSON.parse(await readFile('package.json', 'utf8')).version;
    for (const name of ['SSH2-LICENSE', 'HASH-WASM-LICENSE']) assert.ok((await readFile(path.join(resources, name), 'utf8')).length);
    if (process.platform === 'win32') {
      for (const file of ['cli-runtime/node.exe', 'cli-runtime/NODE-LICENSE', 'util/pagent.exe']) assert.ok((await readFile(path.join(resources, file))).length);
    }
    // Generate the real installed shims. Do not substitute a test helper or runtime.
    const factory = path.join(root, 'shim-factory.cjs');
    await build({ stdin: { contents: "export { shellfoxShims } from './src/main/platform/shellfox-cli'; export { linuxLauncher } from './src/main/platform/linux-cli';", resolveDir: process.cwd() },
      outfile: factory, bundle: true, platform: 'node', format: 'cjs', loader: { '.sh': 'text', '.ps1': 'text' } });
    const { shellfoxShims, linuxLauncher } = require(factory);
    const bin = path.join(root, 'bin'), data = path.join(root, 'user data'); await mkdir(bin);
    const windows = process.platform === 'win32', helper = path.join(resources, 'main', 'cli.cjs');
    const executable = path.join(root, windows ? 'Shellfox.exe' : 'shellfox');
    const options = { executable, userData: data };
    if (windows) {
      const shims = shellfoxShims(options);
      await writeFile(path.join(bin, 'shellfox.cmd'), shims.cmd);
      await writeFile(path.join(bin, 'shellfox.ps1'), shims.powershell);
      assert.ok(shims.cmd.includes(helper));
    } else {
      await writeFile(path.join(bin, 'shellfox'), linuxLauncher({ ...options, executable: process.execPath, cliHelper: helper }));
    }
    const invoke = async args => {
      const file = windows ? 'cmd.exe' : '/bin/sh';
      const argv = windows ? ['/d', '/c', 'shellfox.cmd', ...args] : [path.join(bin, 'shellfox'), ...args];
      try { return { ...await exec(file, argv, { cwd: windows ? bin : root, timeout: 15000 }), code: 0 }; }
      catch (error) { if (typeof error.code !== 'number') throw error; return error; }
    };
    assert.equal((await invoke(['--version'])).stdout.trim(), version);
    const help = await invoke(['--help']); assert.equal(help.code, 0); assert.match(help.stdout, /shellfox cmd --register <name> <script-path>/);
    const script = path.join(root, windows ? 'script with spaces.ps1' : 'script with spaces.sh');
    await writeFile(script, windows ? '[Console]::Write((ConvertTo-Json -Compress -InputObject @($args))); exit 17' : 'printf "%s\\0" "$@"\nexit 17\n');
    const registered = await invoke(['cmd', '--register', 'custom', script]);
    assert.equal(registered.code, 0, registered.stderr + registered.stdout);
    const record = JSON.parse(await readFile(path.join(data, 'cli-commands.json'), 'utf8'));
    assert.deepEqual(record.commands, [{ name: 'custom', path: script }]);
    const args = ['two words', 'literal & echo not-code', '--help', '--register', '--rename', '--rm', '--list', '--run'];
    const result = await invoke(['cmd', 'custom', ...args]); assert.equal(result.code, 17, result.stderr);
    assert.deepEqual(windows ? JSON.parse(result.stdout) : result.stdout.split('\0').slice(0, -1), args);
    if (windows) {
      const literal = ['cmd', '--run', 'custom', '', '"quoted"', "it's", 'a&b', '%PATH%', '--help', 'line\nbreak'];
      const payload = Buffer.from(JSON.stringify(literal)).toString('base64');
      const quote = value => "'" + value.replaceAll("'", "''") + "'";
      const code = `$argv=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('${payload}')) | ConvertFrom-Json; & ${quote(path.join(bin, 'shellfox.ps1'))} @argv; exit $LASTEXITCODE`;
      const ps = await exec('powershell.exe', ['-NoLogo', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', Buffer.from(code, 'utf16le').toString('base64')], { cwd: root, timeout: 15000 }).catch(error => error);
      assert.equal(ps.code, 17, ps.stderr); assert.deepEqual(JSON.parse(ps.stdout), literal.slice(3));
    }
    for (const args of [['cmd', '--list'], ['cmd', '--ls']]) {
      const list = await invoke(args); assert.equal(list.code, 0); assert.ok(list.stdout.includes(JSON.stringify(script)));
    }
    await writeFile(script, windows ? '[Console]::Write("latest file"); exit 0' : 'printf "latest file"\n');
    assert.equal((await invoke(['cmd', '--run', 'custom'])).stdout, 'latest file');
    assert.equal((await invoke(['cmd', '--register', 'CUSTOM', script])).code, 1);
    assert.equal((await invoke(['cmd', '--rename', 'custom', 'renamed'])).code, 0);
    // Custom names share no top-level namespace, even in the release bundle.
    for (const name of ['version', 'start', 'ssh', 'cmd']) {
      assert.equal((await invoke(['cmd', '--register', name, script])).code, 0);
      assert.equal((await invoke(['cmd', name])).stdout, 'latest file');
      const top = await invoke(name === 'version' ? ['version'] : [name, '--help']);
      assert.equal(top.code, 0, top.stderr); assert.equal(top.stderr, '');
      if (name === 'version') assert.equal(top.stdout.trim(), version);
      else assert.notEqual(top.stdout, 'latest file');
      assert.equal((await invoke(['cmd', '--rm', name])).code, 0);
    }
    const builtin = await invoke(['version']); assert.equal(builtin.code, 0);
    assert.equal(builtin.stdout.trim(), version); assert.equal(builtin.stderr, '');
    assert.equal((await invoke(['register', 'obsolete', script])).code, 2);
    assert.equal((await invoke(['commands'])).code, 2);
    assert.equal((await invoke(['list', '--commands'])).code, 2);
    assert.equal((await invoke(['cmd', '--remove', 'renamed'])).code, 0);
    assert.deepEqual(JSON.parse(await readFile(path.join(data, 'cli-commands.json'), 'utf8')).commands, []);
    assert.ok((await readFile(script, 'utf8')).length);
  } finally { await rm(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 }); }
});
