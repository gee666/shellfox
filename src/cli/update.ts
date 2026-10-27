import { mkdir, mkdtemp, rm, unlink } from 'node:fs/promises';
import path from 'node:path';
import { isNewer, LATEST_RELEASE_URL, RELEASES_URL } from '../main/update-check';
import { downloadAsset, parseRelease, readReleaseBody, type UpdateFetch, type UpdateRelease } from '../main/update/self-updater';
import { CliUsageError } from './arguments';
import { CliFailure } from './output';
import { resolveRuntimeContext } from './runtime-context';
import { extractHeadlessArchive } from './update-archive';

import { validateHeadlessInstallation } from './headless-installation';
import { acquireUpdateLock, activateHeadlessUpdate, activeHeadlessInstallation, headlessUpdateAnchor } from './update-activation';
export interface HeadlessUpdateResult {
  current: string; latest: string | null; available: boolean; phase: 'current' | 'available' | 'installed';
  installation: string | null; restartRequired: boolean; message: string;
}
export interface HeadlessUpdateOptions {
  entry: string; current: string; check: boolean; platform?: string; arch?: string; fetch?: UpdateFetch;
  extract?: typeof extractHeadlessArchive;
}
export function parseUpdateArgs(args: string[], host?: string): { check: boolean } {
  if (host && host !== 'local') throw new CliUsageError('update only updates this local installation. Run it on the target machine.');
  if (args.length > 1 || (args[0] !== undefined && args[0] !== '--check')) throw new CliUsageError('Usage: shellfox update [--check] [--json]');
  return { check: args[0] === '--check' };
}
/** Select a verified version for future starts without changing live hosts or startup configuration. */
export async function runHeadlessUpdate(options: HeadlessUpdateOptions): Promise<HeadlessUpdateResult> {
  const platform = options.platform ?? process.platform, arch = options.arch ?? process.arch;
  if (!['win32', 'darwin', 'linux'].includes(platform) || !['x64', 'arm64'].includes(arch)) throw new CliFailure('UNSUPPORTED', `No headless update is published for ${platform}/${arch}. See ${RELEASES_URL}`);
  const context = await resolveRuntimeContext(options.entry, {});
  if (!context.headless || !context.runtimePath) throw new CliFailure('UNSUPPORTED', 'Headless updates require an extracted headless release package. Update source checkouts through your checkout, or use the desktop updater.');
  await validateHeadlessInstallation(context.runtimePath, options.current, platform, arch);
  const fetch = options.fetch ?? globalThis.fetch;
  let release: UpdateRelease | null;
  try {
    const response = await fetch(LATEST_RELEASE_URL, { headers: { Accept: 'application/vnd.github+json', 'User-Agent': 'shellfox-updater' }, signal: AbortSignal.timeout(15_000), redirect: 'error' });
    if (response.status === 404) return { current: options.current, latest: null, available: false, phase: 'current', installation: null, restartRequired: false, message: 'No published Shellfox release yet.' };
    if (!response.ok) throw new Error(`Release lookup failed with HTTP ${response.status}.`);
    release = parseRelease(await readReleaseBody(response), version => `shellfox-headless-${version}-${platform}-${arch}.${platform === 'win32' ? 'zip' : 'tar.gz'}`);
    if (!release) throw new Error('Invalid stable release metadata.');
  } catch (cause) {
    throw new CliFailure('UPDATE_FAILED', `Could not check the latest Shellfox release. ${cause instanceof Error ? cause.message : 'Release lookup failed.'} Check your connection or see ${RELEASES_URL}`, true);
  }
  const available = isNewer(release.version, options.current);
  const result: HeadlessUpdateResult = { current: options.current, latest: release.version, available, phase: available ? 'available' : 'current', installation: null, restartRequired: false,
    message: available ? `Shellfox ${release.version} is available. Run shellfox update to install it for future launches.` : `Shellfox is up to date (${options.current}).` };
  if (!available) return result;
  if (!release.asset) throw new CliFailure('UNSUPPORTED', `Release ${release.version} has no unique, SHA-256-verified headless archive for ${platform}/${arch}. See ${RELEASES_URL}`);
  if (options.check) return result;
  const anchor = await headlessUpdateAnchor(context.runtimePath);
  const releaseLock = await acquireUpdateLock(anchor);
  let work: string | undefined;
  try {
    if (await activeHeadlessInstallation(context.runtimePath)) throw new Error('Another update was activated during this check. Run shellfox update again.');
    work = await mkdtemp(path.join(path.dirname(anchor), `shellfox-headless-${release.version}-${platform}-${arch}-`));
    const archive = path.join(work, release.asset.name);
    await downloadAsset(fetch, release.asset, archive, () => {}, AbortSignal.timeout(30 * 60 * 1000));
    const destination = path.join(work, 'installation');
    await mkdir(destination);
    const installation = await (options.extract ?? extractHeadlessArchive)(archive, destination, `shellfox-headless-${platform}-${arch}`, platform);
    await validateHeadlessInstallation(installation, release.version, platform, arch);
    await unlink(archive);
    await activateHeadlessUpdate(anchor, installation);
    return { ...result, phase: 'installed', installation, restartRequired: true,
      message: `Shellfox ${release.version} installed for future launches. Keep using the original launcher and service paths at ${anchor}. The active host and its terminals are unchanged. Restart the host explicitly through its existing startup manager, or stop a foreground host with --user-data <existing-data-directory> host stop --yes and run host run with the same data directory. Restarting ends its terminals and running commands. Keep both ${anchor} and ${installation}; they are needed for future launches.` };
  } catch (cause) {
    let retained = '';
    if (work) await rm(work, { recursive: true, force: true }).catch(() => { retained = ` Remove incomplete staging files at ${work} before retrying.`; });
    throw new CliFailure('UPDATE_FAILED', `Could not install the headless update.${retained} ${cause instanceof Error ? cause.message : 'Update staging failed.'} The active version and running hosts were not changed. Check free space, write access to the original installation and its parent, and tar availability. See ${RELEASES_URL}`, true);
  } finally { await releaseLock(); }
}
