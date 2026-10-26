// Compile the production snapshot/cleanup functions against deterministic OS stubs.
// This tests C control flow, not macOS ABI, libproc, signals or actual PTYs.
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const root = fileURLToPath(new URL('../', import.meta.url));
const directory = path.join(root, 'tmp', 'darwin-supervisor-model', randomUUID());
mkdirSync(directory, { recursive: true });
const source = readFileSync(process.argv[2] ?? path.join(root, 'src/main/terminal/native/darwin-terminal-supervisor.c'), 'utf8');
function extract(name) {
  const type = name === 'observe_shell' ? 'void' : 'bool';
  const start = source.indexOf(`static ${type} ${name}(`);
  if (start < 0) throw Error(`Missing production function ${name}`);
  const end = source.indexOf('\n}\n', start);
  if (end < 0) throw Error(`Unterminated production function ${name}`);
  return source.slice(start, end + 3);
}
const names = ['info', 'same_birth', 'readable_member', 'session_snapshot', 'child_status', 'exited_event', 'observe_shell', 'confirm_shell_exit', 'cleanup_session'];
if (source.includes('static bool snapshot_failure(')) names.splice(3, 0, 'snapshot_failure');
const constants = source.match(/^#define (?:MAX_PIDS|MAX_GROUPS|FREEZE_SECONDS|CONFIRM_SECONDS) .+$/gm);
if (constants?.length !== 4) throw Error('Missing production bounds');
writeFileSync(path.join(directory, 'production.h'), constants.join('\n') + '\n' + names.map(extract).join('\n'));
const output = path.join(directory, 'model-test');
for (const [file, args] of [
  [process.env.CC ?? (process.platform === 'darwin' ? 'clang' : 'cc'), ['-std=c11', '-O2', '-Wall', '-Wextra', '-Werror', '-I', directory, path.join(root, 'src/main/terminal/native/tests/darwin-supervisor-model.c'), '-o', output]],
  [output, []],
]) {
  const result = spawnSync(file, args, { cwd: root, timeout: 30000, encoding: 'utf8', env: { ...process.env, TMPDIR: directory, TMP: directory, TEMP: directory } });
  process.stdout.write(result.stdout ?? ''); process.stderr.write(result.stderr ?? '');
  if (result.error || result.status !== 0) throw Error(`Darwin supervisor C model failed: ${file}; status=${result.status}; ${result.error?.message ?? ''}`);
}
