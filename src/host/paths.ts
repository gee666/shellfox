import path from 'node:path';
import type { TrackingHelperOptions } from '../main/terminal/tracking';
import { pinnedLegacyHelper, prepareWindowsHelper, type HelperLaunch } from './stable-helper';

declare const __TAILSCALE_HELPER_SHA256__: string;

export interface HostResourceOptions { resourcesPath?: string; projectRoot?: string; packaged?: boolean }
export interface NetworkHelperOptions extends HostResourceOptions {
  /** Explicit legacy/operator fixture. Verified in place, never copied or given new CLI flags. */
  helperPath?: string;
  helperSha256?: string;
}

/** Explicit helpers need their operator's digest, never an inferred hash of a mutable file. */
export async function hostNetworkHelper(options: NetworkHelperOptions = {}): Promise<HelperLaunch> {
  const candidate = options.helperPath ?? hostResourcePaths(options).tailscale;
  if (process.platform !== 'win32') return { executable: candidate, args: [] };
  if (options.helperPath) return pinnedLegacyHelper(candidate, options.helperSha256 ?? '');
  const expectedSha256 = typeof __TAILSCALE_HELPER_SHA256__ === 'string' ? __TAILSCALE_HELPER_SHA256__ : undefined;
  if (!expectedSha256) throw new Error('Windows network helper has no trusted build SHA256. Rebuild Shellfox or supply an explicit helper digest.');
  return prepareWindowsHelper({ candidate, expectedSha256, localAppData: process.env.LOCALAPPDATA ?? '', arch: process.arch });
}

export function hostResourcePaths(options: HostResourceOptions = {}): { helpers: TrackingHelperOptions; tailscale: string } {
  const runtime = process as NodeJS.Process & { resourcesPath?: string; defaultApp?: boolean };
  const electronResources = runtime.versions.electron && !runtime.defaultApp ? runtime.resourcesPath : undefined;
  const resourcesPath = options.resourcesPath ?? electronResources ?? process.env.SHELLFOX_RESOURCES_PATH;
  const projectRoot = path.resolve(options.projectRoot ?? process.env.SHELLFOX_PROJECT_ROOT ?? process.cwd());
  const packaged = options.packaged ?? !!resourcesPath;
  if (packaged && !resourcesPath) throw new Error('Packaged host requires an explicit resourcesPath or SHELLFOX_RESOURCES_PATH.');
  const name = 'shellfox-tailscale' + (process.platform === 'win32' ? '.exe' : '');
  // This is also used for offline ownership/permissions modes, never as a Windows network launch path.
  return { helpers: { resourcesPath, projectRoot, packaged }, tailscale: resourcesPath && packaged ? path.join(resourcesPath, 'tailscale', name) : path.join(projectRoot, 'tmp/tailscale', process.platform + '-' + process.arch, name) };
}
