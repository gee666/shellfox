import { spawnSync } from 'node:child_process';
import { mkdirSync } from 'node:fs';
import path from 'node:path';

/** Windows Firewall keys approval to the executable path, not the Go package.
 * Compile tests before running them so go-build's random paths never open sockets.
 */
export function runGoTests({ command, cwd, env, root, packages = ['./...'], timeoutSeconds = 60,
  platform = process.platform, arch = process.arch }, execute = spawnSync) {
  const run = (file, args, capture = false, directory = cwd) => {
    console.log('[go-test]', file, ...args);
    const result = execute(file, args, { cwd: directory, env, timeout: 600_000, windowsHide: true,
      encoding: 'utf8', stdio: capture ? 'pipe' : 'inherit', maxBuffer: 32 * 1024 * 1024 });
    if (result.error || result.status !== 0) throw new Error(`Go test command failed: ${result.error?.message ?? result.stderr ?? file}`);
    return result.stdout;
  };
  if (platform !== 'win32') {
    run(command, ['test', '-mod=readonly', '-count=1', `-timeout=${timeoutSeconds}s`, ...packages]);
    return;
  }
  const listed = run(command, ['list', '-mod=readonly', '-f', '{{if or .TestGoFiles .XTestGoFiles}}{{.ImportPath}}\t{{.Dir}}{{end}}', ...packages], true);
  const imports = new Map(listed.trim().split(/\r?\n/).filter(Boolean).map(line => line.split('\t')));
  for (const [name, packageDirectory] of imports) {
    if (!packageDirectory || !path.isAbsolute(packageDirectory) || !/^[a-zA-Z0-9][a-zA-Z0-9._~/-]*$/.test(name) || name.split('/').some(part => !part || part === '.' || part === '..')) {
      throw new Error('Invalid Go test package path.');
    }
    const directory = path.join(root, 'tmp', 'go-tests', `${platform}-${arch}`, encodeURIComponent(name));
    mkdirSync(directory, { recursive: true });
    const executable = path.join(directory, path.posix.basename(name) + '.test.exe');
    // Never execute go-build output or fall back to a new path when this one is busy.
    run(command, ['test', '-mod=readonly', '-trimpath', '-buildvcs=false', '-c', '-o', executable, name]);
    run(executable, ['-test.count=1', `-test.timeout=${timeoutSeconds}s`], false, packageDirectory);
  }
}
