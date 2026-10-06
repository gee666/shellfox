import path from 'node:path';
// Packaging contract only; protocols are implemented/validated by backend owners.
export const darwinHelpers = [
  { sourceFile: 'darwin-process-snapshot.c', basename: 'shellfox-process-snapshot',
    source: 'libproc+numeric-sysctl', termination: false },
  { sourceFile: 'darwin-terminal-supervisor.c', basename: 'shellfox-terminal-supervisor',
    termination: true },
];
export function darwinCompilerArgs(root, arch, helper) {
  if (!['x64', 'arm64'].includes(arch)) throw new Error('Unsupported native Darwin architecture.');
  return ['--sdk', 'macosx', 'clang', '-std=c11', '-O2', '-Wall', '-Wextra', '-Werror',
    '-arch', arch === 'x64' ? 'x86_64' : 'arm64', '-mmacosx-version-min=13.0',
    path.join(root, 'src/main/terminal/native', helper.sourceFile), '-lproc', '-o',
    path.join(root, 'tmp/terminal-native', 'darwin-' + arch, helper.basename)];
}
