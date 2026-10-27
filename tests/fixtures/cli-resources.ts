import { copyFile, mkdir } from 'node:fs/promises';
import path from 'node:path';

/** Forge normally stages these resources beside the standalone CLI. */
export async function stageCliResources(projectRoot: string): Promise<void> {
  if (process.platform !== 'win32') return;
  const destination = path.join(projectRoot, 'tmp/build-test/tailscale');
  await mkdir(destination, { recursive: true });
  await copyFile(path.join(projectRoot, `tmp/tailscale/win32-${process.arch}/shellfox-tailscale.exe`), path.join(destination, 'shellfox-tailscale.exe'));
}
