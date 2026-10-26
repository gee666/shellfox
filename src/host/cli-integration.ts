import path from 'node:path';
import { access, realpath } from 'node:fs/promises';
import { constants } from 'node:fs';
import { success, failure } from '../shared/contracts';
import type { CliPort } from '../main/platform/shellfox-cli';

/** Advertise only a verified bundled launcher. Never edits the user's global PATH. */
export async function bundledHostCli(runtimePath = process.env.SHELLFOX_RUNTIME_PATH): Promise<CliPort | undefined> {
  if (!runtimePath) return undefined;
  let root: string;
  try { root = await realpath(path.resolve(runtimePath)); } catch { return undefined; }
  const executable = process.platform === 'win32' ? 'node.exe' : 'node';
  const launcher = process.platform === 'win32' ? 'shellfox.cmd' : 'shellfox';
  try {
    await access(path.join(root, launcher), process.platform === 'win32' ? constants.F_OK : constants.X_OK);
    await access(path.join(root, 'bin', executable), process.platform === 'win32' ? constants.F_OK : constants.X_OK);
    await access(path.join(root, 'app', 'main.cjs'), constants.R_OK);
  } catch { return undefined; }
  const state = { supported: false, installed: true, command: 'shellfox', reason: 'The bundled CLI is available in host terminals. Global PATH changes are managed by the CLI installer.' };
  return { binDir: root, readOnly: true, get: async () => success(state), set: async () => failure('UNSUPPORTED', state.reason) };
}
