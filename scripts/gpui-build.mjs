import path from 'node:path';
import { randomUUID } from 'node:crypto';

// -o leaves Cargo's regular executable alone, including a loaded user window.
export function privateBuildPlan(root, { release = false, packageName = 'shellfox-desktop', binName = 'shellfox-gpui-experiment', nonce = randomUUID() } = {}) {
  if (!path.isAbsolute(root) || !/^[a-z0-9-]+$/i.test(nonce)) throw new Error('Invalid private build directory.');
  const directory = path.join(root, 'tmp', 'gpui-builds', nonce);
  const executable = path.join(directory, `${binName}${process.platform === 'win32' ? '.exe' : ''}`);
  const args = ['rustc', '--locked', '-p', packageName, '--bin', binName];
  if (release) args.push('--release');
  args.push('--', '-o', executable);
  return { directory, executable, args };
}
