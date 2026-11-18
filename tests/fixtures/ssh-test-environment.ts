import path from 'node:path';
/** Synthetic test context only; never inherits agent sockets or personal profiles. */
export function sshTestEnvironment(directory: string, inherited: NodeJS.ProcessEnv = process.env): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = {};
  for (const key of ['SystemRoot', 'WINDIR', 'PATH', 'Path', 'PATHEXT', 'ProgramFiles', 'ProgramFiles(x86)']) {
    if (inherited[key] !== undefined) env[key] = inherited[key];
  }
  const root = path.resolve(directory);
  return { ...env, HOME: root, USERPROFILE: root, APPDATA: root, LOCALAPPDATA: root, TEMP: root, TMP: root, TMPDIR: root, NO_COLOR: '1' };
}
