import path from 'node:path';
import type { TrackingHelperOptions } from '../main/terminal/tracking';

export interface HostResourceOptions { resourcesPath?: string; projectRoot?: string; packaged?: boolean }
export function hostResourcePaths(options: HostResourceOptions = {}): { helpers: TrackingHelperOptions; tailscale: string } {
  const runtime = process as NodeJS.Process & { resourcesPath?: string; defaultApp?: boolean };
  const electronResources = runtime.versions.electron && !runtime.defaultApp ? runtime.resourcesPath : undefined;
  const resourcesPath = options.resourcesPath ?? electronResources ?? process.env.SHELLFOX_RESOURCES_PATH;
  const projectRoot = path.resolve(options.projectRoot ?? process.env.SHELLFOX_PROJECT_ROOT ?? process.cwd());
  const packaged = options.packaged ?? !!resourcesPath;
  if (packaged && !resourcesPath) throw new Error('Packaged host requires an explicit resourcesPath or SHELLFOX_RESOURCES_PATH.');
  const name = 'shellfox-tailscale' + (process.platform === 'win32' ? '.exe' : '');
  return { helpers: { resourcesPath, projectRoot, packaged }, tailscale: resourcesPath && packaged ? path.join(resourcesPath, 'tailscale', name) : path.join(projectRoot, 'tmp/tailscale', process.platform + '-' + process.arch, name) };
}
