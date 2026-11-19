import { spawnSync } from 'node:child_process';
import { mkdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const output = fileURLToPath(new URL('../tmp/dev-helper/gotest/', import.meta.url));

export function runGoTests({ go, cwd, env, packages, testTimeout,
  platform = process.platform, spawn = spawnSync, directory = output }) {
  function run(command, args, { capture = false, dir = cwd, timeout = 600_000 } = {}) {
    if (!capture) console.log('[go-test]', command, ...args);
    const result = spawn(command, args, { cwd: dir, env, stdio: capture ? 'pipe' : 'inherit',
      encoding: 'utf8', timeout, windowsHide: true });
    if (result.error || result.status !== 0) {
      throw new Error(`Go test command failed (${result.status ?? result.signal ?? 'spawn'}): ${command} ${args.join(' ')}`
        + (result.stderr ? `\n${result.stderr.trim()}` : ''), { cause: result.error });
    }
    return result.stdout;
  }

  const buildFlags = ['-mod=readonly'];
  if (platform !== 'win32') {
    const timeoutFlag = testTimeout ? [`-timeout=${testTimeout}`] : [];
    run(go, ['test', ...buildFlags, '-count=1', ...timeoutFlag, ...packages]);
    return;
  }

  // Windows Firewall remembers executable paths. Keep one test binary per package.
  const format = '{{if or .TestGoFiles .XTestGoFiles}}{{.ImportPath}}\t{{.Dir}}{{end}}';
  const rows = run(go, ['list', ...buildFlags, '-f', format, ...packages], { capture: true });
  mkdirSync(directory, { recursive: true });
  const names = new Set();
  for (const row of rows.trim().split(/\r?\n/).filter(Boolean)) {
    const [pkg, dir] = row.split('\t');
    const name = `${path.posix.basename(pkg)}.test.exe`;
    if (names.has(name.toLowerCase())) throw new Error(`Duplicate Go test executable name: ${name}`);
    names.add(name.toLowerCase());
    const executable = path.join(directory, name);
    run(go, ['test', ...buildFlags, '-c', '-o', executable, pkg]);
    // Like go test, run each binary from its package directory for relative fixtures.
    run(executable, ['-test.count=1', `-test.timeout=${testTimeout ?? '10m'}`, '-test.paniconexit0'], { dir, timeout: 610_000 });
  }
}
