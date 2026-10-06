import { spawn } from 'node:child_process';
import path from 'node:path';
import { env, root, testMain, executable } from './electron';
export function cli(data: string, args: string[] = [], packaged = false, overrides: NodeJS.ProcessEnv = {}, backend: 'fake' | 'real' = packaged ? 'real' : 'fake') {
  const child = spawn(packaged ? executable : path.join(root, 'node_modules/electron/dist/electron.exe'), [...(packaged ? [] : [testMain]), '--test-user-data', data, '--test-backend', backend, ...args], { cwd: root, env: { ...env(), ...overrides }, shell: false, windowsHide: true, stdio: 'pipe' });
  child.stdin.end();
  let stderr = ''; child.stderr.on('data', b => { stderr += b.toString(); }); child.stdout.resume();
  return new Promise<{ code: number | null; stderr: string }>((resolve, reject) => {
    const timer = setTimeout(() => { child.kill(); reject(new Error('Owned CLI process exceeded 20s deadline')); }, 20000);
    child.on('error', error => { clearTimeout(timer); reject(error); });
    child.on('exit', code => { clearTimeout(timer); resolve({ code, stderr }); });
  });
}
