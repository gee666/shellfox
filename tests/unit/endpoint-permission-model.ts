import { lstat } from 'node:fs/promises';
import path from 'node:path';

type Kind = 'directory' | 'file';
type Action = 'protect' | 'check';
type Reply = { stdout: string; stderr: string };
export type PermissionStep = { kind: Kind; action: Action; reply: Reply | Error };
interface Fixture {
  helper: string;
  protectedDirectory?: string;
  protectedFiles: Set<string>;
  remaining: number;
  steps?: PermissionStep[];
}
export class PermissionModelBoundaryError extends Error {}
const fixtures = new Map<string, Fixture>();
const projectTmp = path.resolve('tmp');
const accepted: Reply = { stdout: '{"ok":true}\n', stderr: '' };
const denied: Reply = { stdout: '{"ok":false}\n', stderr: '' };

function expectedHelper(): string {
  const resource: unknown = Reflect.get(process, 'resourcesPath');
  const resources = process.versions.electron && !Reflect.get(process, 'defaultApp') && typeof resource === 'string'
    ? resource : process.env.SHELLFOX_RESOURCES_PATH;
  const name = 'shellfox-tailscale' + (process.platform === 'win32' ? '.exe' : '');
  return resources ? path.join(resources, 'tailscale', name)
    : path.join(path.resolve(process.env.SHELLFOX_PROJECT_ROOT ?? process.cwd()), 'tmp/tailscale', process.platform + '-' + process.arch, name);
}

/** Model only. Each caller names its project-tmp root and endpoint directories. */
export function registerEndpointRoot(root: string, directories: readonly string[], script?: readonly PermissionStep[]): void {
  if (!path.isAbsolute(root) || path.dirname(root) !== projectTmp || !directories.length || directories.length > 2 ||
    fixtures.size + directories.length > 512 || (script && directories.length !== 1) || (script?.length ?? 0) > 16) {
    throw new PermissionModelBoundaryError('Bounded project-tmp fixture required');
  }
  const paths = directories.map(name => path.join(root, name));
  if (directories.some(name => !name || name === '.' || name === '..' || /[/\\:\0]/.test(name)) ||
    new Set(paths).size !== paths.length || paths.some(directory => fixtures.has(directory))) {
    throw new PermissionModelBoundaryError('Unique endpoint fixture paths required');
  }
  if (script?.some(({ reply }) => !(reply instanceof Error) && (reply.stdout.length > 1024 || reply.stderr.length > 1024))) {
    throw new PermissionModelBoundaryError('Model reply bound');
  }
  for (const directory of paths) fixtures.set(directory, {
    helper: expectedHelper(), protectedFiles: new Set(), remaining: 512, steps: script && [...script],
  });
}

export function finishPermissionFixtures(): void {
  const pending = [...fixtures.values()].some(fixture => fixture.steps?.length);
  fixtures.clear();
  if (pending) throw new Error('Unconsumed endpoint permission steps');
}

export async function modelEndpointCommand(file: unknown, args: unknown, options: unknown): Promise<Reply> {
  if (typeof file !== 'string' || !Array.isArray(args) || args.length !== 4) throw new PermissionModelBoundaryError('Unregistered app command');
  const [verb, target, kind, action]: unknown[] = args;
  if (verb !== '--private-permissions' || typeof target !== 'string' || !path.isAbsolute(target) || path.normalize(target) !== target ||
    (kind !== 'directory' && kind !== 'file') || (action !== 'protect' && action !== 'check')) {
    throw new PermissionModelBoundaryError('Unregistered permission operation');
  }
  const directory = kind === 'directory' ? target : path.dirname(target);
  const fixture = fixtures.get(directory);
  if (fixture?.helper !== file || fixture.remaining <= 0 || !matchesOptions(options)) {
    throw new PermissionModelBoundaryError('Unregistered permission fixture/options');
  }
  const filename = path.basename(target);
  if (kind === 'file' && !(action === 'protect' ? /^[0-9a-f]{32}\.json$/.test(filename) : filename === 'endpoint.json')) {
    throw new PermissionModelBoundaryError('Unregistered permission filename');
  }
  fixture.remaining--;
  if (fixture.steps) {
    const step = fixture.steps[0];
    if (step?.kind !== kind || step.action !== action) throw new PermissionModelBoundaryError('Unscripted permission step');
    fixture.steps.shift();
    if (step.reply instanceof Error) throw step.reply;
    return step.reply;
  }
  return modelPublication(fixture, directory, target, kind, action);
}

function matchesOptions(options: unknown): boolean {
  return typeof options === 'object' && options !== null && Object.keys(options).length === 3 &&
    'windowsHide' in options && options.windowsHide === true && 'timeout' in options && options.timeout === 8000 &&
    'maxBuffer' in options && options.maxBuffer === 1024;
}

async function fixtureIdentity(target: string, kind: Kind): Promise<string | undefined> {
  const info = await lstat(target, { bigint: true });
  if ((kind === 'directory' ? !info.isDirectory() : !info.isFile() || info.nlink !== 1n) || info.ino === 0n) return;
  // Fixture correlation across rename only, not security or ownership evidence.
  return `${info.dev}:${info.ino}:${info.birthtimeNs}`;
}

async function modelPublication(fixture: Fixture, directory: string, target: string, kind: Kind, action: Action): Promise<Reply> {
  const identity = await fixtureIdentity(target, kind);
  if (!identity) return denied;
  if (kind === 'directory') {
    if (action === 'protect' && !fixture.protectedDirectory) fixture.protectedDirectory = identity;
    return fixture.protectedDirectory === identity ? accepted : denied;
  }
  if (!fixture.protectedDirectory || await fixtureIdentity(directory, 'directory') !== fixture.protectedDirectory) return denied;
  if (action === 'protect') fixture.protectedFiles.add(identity);
  return fixture.protectedFiles.has(identity) ? accepted : denied;
}
